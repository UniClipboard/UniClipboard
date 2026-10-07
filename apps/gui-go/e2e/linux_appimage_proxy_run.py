#!/usr/bin/env python3
"""System-proxy E2E of the self-contained AppImage (slice 17c12, docs/architecture/gui-go-linux-appimage-system-proxy.md).

  linux_appimage_proxy_run.py --out DIR --appimage X.AppImage --manifest package-manifest.json [--nonportable] [--require] [--scenarios a,b,...]

Real AppImage, real WebView of the shared frontend, real release daemon, a REAL forward proxy (tinyproxy, access log) and a controlled HTTPS target that is NOT on
loopback (`target.test` -> the container's own interface address; its CA is installed through the container's own trust mechanism). Inside an `--internal` container
network nothing can leave. Each scenario starts the GUI once with its own proxy configuration. The route of one request is read from TWO logs, one hostname and one
log window per client (a CONNECT line names its host only; dev2 lost this and attributed curl's CONNECT to the WebView):
  direct    the target saw the request, the proxy never named the host
  proxied   the proxy named the host and the target saw the request through it
  refused   the proxy answered 403 (deny mode), the target saw nothing
  failed    neither log has it (a dead proxy: the request neither escaped to the target nor reached the proxy)
Two separate results are written, never merged:
  passed            the CONTROLS and observations hold (the fixture is valid: curl with the same configuration reaches/is refused by the proxy, the loopback detection
                    control is seen, the daemon connection of the WebView is on loopback). A baseline run has passed=true and records how the WebView actually routed.
  functionalPassed  the REQUIREMENTS hold: with the proxy configured the real WebView goes through the proxy (allow), is refused by it (deny), FAILS without escaping
                    to the target when the proxy is unreachable (dead), and the local daemon stays usable on loopback. Only evaluated with --require.
Modes: portable (default; no Secret Service) or --nonportable (the release way: real HOME, user session bus + unlocked Secret Service in the same container).
Nothing here talks to a production service; the proxy never forwards anything but the controlled target.
"""
import argparse
import http.server
import json
import os
import pwd
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_appimage_portable_run import UserRun, USER, as_user, environ_of, stop, wait_daemon  # noqa: E402
from linux_appimage_run import DISPLAY, PASSPHRASE, maps_of, pid_alive, procs, sha256, start_xvfb, wait_panel_ready  # noqa: E402
from linux_appimage_tls_run import Reports, StopScenario, install_trust, run_cmd, wait_new_daemon  # noqa: E402

TARGET_NAME = 'target.test'
WV_HOST, CURL_HOST = 'webview-probe.test', 'curl-probe.test'  # one hostname per client
HOSTS = (TARGET_NAME, WV_HOST, CURL_HOST)
LOOPBACK = re.compile(r'(127\.\d+\.\d+\.\d+|localhost|\[?::1\]?)')
REQ = re.compile(r'Request \(file descriptor \d+\): (\w+) (\S+)')
BUS_DIR = Path('/bus')
PROXY_MODULES = re.compile(r'(libgiognomeproxy|libgiolibproxy|libdconfsettings|libproxy|libpxbackend|libduktape)')


def make_target_cert(workdir):
    """Throwaway CA + a leaf for the probe hostnames (openssl CLI)."""
    d = workdir / 'target-pki'
    d.mkdir()
    ca_key, ca_crt, key, csr, crt = (d / x for x in ('ca.key', 'ca.crt', 'leaf.key', 'leaf.csr', 'leaf.crt'))
    for s in (['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(ca_key), '-out', str(ca_crt), '-days', '2', '-subj', '/CN=uc-17c12-test-ca',
               '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'],
              ['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key), '-out', str(csr), '-subj', f'/CN={TARGET_NAME}']):
        r = run_cmd(s)
        if r.returncode:
            raise RuntimeError(f'openssl failed: {s[:3]} {r.stderr}')
    (d / 'ext.cnf').write_text('subjectAltName=' + ','.join(f'DNS:{h}' for h in HOSTS) + '\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nkeyUsage=digitalSignature,keyEncipherment\n')
    r = run_cmd(['openssl', 'x509', '-req', '-in', str(csr), '-CA', str(ca_crt), '-CAkey', str(ca_key), '-CAcreateserial', '-out', str(crt), '-days', '2', '-extfile', str(d / 'ext.cnf')])
    if r.returncode:
        raise RuntimeError('openssl x509 failed: ' + r.stderr)
    return {'ca': ca_crt, 'cert': crt, 'key': key}


class Target:
    """HTTPS target bound to the container's interface address (never loopback), port 443. Records every request line, the peer address and failed handshakes."""

    def __init__(self, material, address):
        self.requests, self.handshake_failures, self.address = [], [], address
        outer = self
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(str(material['cert']), str(material['key']))

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                outer.requests.append({'t': time.time(), 'path': self.path, 'peer': self.client_address[0], 'host': self.headers.get('Host'), 'ua': self.headers.get('User-Agent', ''),
                                       'origin': self.headers.get('Origin')})
                body = b'target-ok'
                self.send_response(200)
                self.send_header('Content-Type', 'text/plain')
                self.send_header('Access-Control-Allow-Origin', '*')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        class Server(http.server.ThreadingHTTPServer):
            def get_request(self):
                sock, addr = self.socket.accept()
                try:
                    return ctx.wrap_socket(sock, server_side=True), addr
                except (ssl.SSLError, OSError) as e:
                    outer.handshake_failures.append(str(e))
                    sock.close()
                    raise OSError('handshake failed') from e

            def handle_error(self, request, client_address):
                pass

        self.server = Server((address, 443), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def saw(self, nonce):
        return [r for r in self.requests if nonce in r['path']]


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


class Proxy:
    """tinyproxy (the distribution's package). mode 'allow' forwards; 'deny' refuses everything (default-deny filter, 403) and still logs the request."""

    def __init__(self, tag, mode):
        self.tag, self.mode, self.port = tag, mode, free_port()
        # tinyproxy drops to `nobody`: its directory must be traversable by it (the sandbox is the GUI user's 0700 directory: the first run logged nothing, dev1)
        self.dir = Path(tempfile.mkdtemp(prefix=f'uc-proxy-{tag}-', dir='/var/tmp'))
        self.dir.chmod(0o755)
        self.log = self.dir / 'tinyproxy.log'
        self.log.write_text('')
        self.log.chmod(0o666)
        conf = ['User nobody', 'Group nogroup', f'Port {self.port}', 'Listen 127.0.0.1', f'LogFile "{self.log}"', 'LogLevel Info', 'Timeout 600', 'MaxClients 100',
                'ConnectPort 443', 'ConnectPort 80', 'DisableViaHeader No']
        if mode == 'deny':
            empty = self.dir / 'filter'
            empty.write_text('')
            conf += [f'Filter "{empty}"', 'FilterDefaultDeny Yes']
        (self.dir / 'tinyproxy.conf').write_text('\n'.join(conf) + '\n')
        self.proc = subprocess.Popen(['tinyproxy', '-d', '-c', str(self.dir / 'tinyproxy.conf')], stdout=(self.dir / 'stdout.log').open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            try:
                socket.create_connection(('127.0.0.1', self.port), timeout=1).close()
                break
            except OSError:
                time.sleep(.2)
        else:
            raise RuntimeError(f'tinyproxy {tag} did not start: ' + (self.dir / 'stdout.log').read_text())

    def outage(self):
        """P7: the proxy process goes away (connections refused on its port)."""
        self.stop()

    def resume(self):
        """P7: a new tinyproxy on the SAME port and configuration (the log keeps growing)."""
        self.proc = subprocess.Popen(['tinyproxy', '-d', '-c', str(self.dir / 'tinyproxy.conf')], stdout=(self.dir / 'stdout.log').open('a'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            try:
                socket.create_connection(('127.0.0.1', self.port), timeout=1).close()
                return
            except OSError:
                time.sleep(.2)
        raise RuntimeError('tinyproxy did not resume: ' + (self.dir / 'stdout.log').read_text())

    def lines(self):
        return [l for l in self.log.read_text(errors='replace').splitlines() if l.strip()]

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def request_targets(lines):
    """(method, target) of the proxy's request lines; only these name where a client wanted to go (`Connect (file descriptor N): 127.0.0.1` is the CLIENT's address)."""
    return [m.groups() for m in (REQ.search(l) for l in lines) if m]


def classify(host, nonce, window, target):
    """Route of ONE client's request, from its own proxy-log window and its own hostname (see the module docstring)."""
    named = [l for l in window if REQ.search(l) and host in REQ.search(l).group(2)]
    refusals = [l for l in window if 'refused' in l.lower() and f'"{host}"' in l]
    saw = target.saw(nonce)
    if refusals and not saw:
        route = 'refused'
    elif named and saw:
        route = 'proxied'
    elif named:
        route = 'proxied-no-delivery'
    elif saw:
        route = 'direct'
    else:
        route = 'failed'
    return {'host': host, 'targetSaw': len(saw), 'proxyLinesNamingHost': named, 'proxyRefusals': refusals, 'route': route}


def sockets_of(names):
    """(process name, pid, local, peer) of every established TCP socket owned by a process whose name starts with one of `names` (root: `ss -p`)."""
    out = run_cmd(['ss', '-tnpH', 'state', 'established']).stdout
    rows = []
    for l in out.splitlines():
        m = re.match(r'\s*\d+\s+\d+\s+(\S+)\s+(\S+)\s+users:\(\("([^"]+)",pid=(\d+)', l)
        if m and any(m.group(3).startswith(n) for n in names):
            rows.append({'proc': m.group(3), 'pid': int(m.group(4)), 'local': m.group(1), 'peer': m.group(2)})
    return rows


def curl_as_user(env, url):
    r = as_user(['curl', '-sS', '-o', '/dev/null', '-w', '%{http_code}', '--max-time', '20', url], env, timeout=40)
    return {'rc': r.returncode, 'code': r.stdout.strip(), 'err': r.stderr.strip()[-200:]}


def write_dconf_proxy(port, where, target_app, nonportable):
    """GNOME's proxy settings (manual proxy, default ignore-hosts), compiled with the distribution's own `dconf compile` (no bus needed).
      user   the user's database ~/.config/dconf/user under the REAL home (what GNOME Settings writes). In portable mode the AppImage's HOME is redirected to
             <AppImage>.home and cannot see it (F7): that is an observation there; in --nonportable it is the real user scenario.
      ph     CAUSAL CONTROL (portable only): the same database inside the portable HOME. Tells "the resolver does not work" from "the config is not visible".
      sys    an administrator's system database /etc/dconf/db/local + /etc/dconf/profile/user (visible to every HOME): independent causal control.
    The host-side control is the distribution's own libproxy CLI `proxy`, run with the HOME the GUI will have."""
    account = pwd.getpwnam(USER)
    real = Path(account.pw_dir)
    portable_home = Path(str(target_app) + '.home')
    db_home = portable_home if where == 'ph' else real
    gui_home = real if nonportable else portable_home  # the HOME the GUI process (and the AppImage's runtime) really has
    kdir = Path(tempfile.mkdtemp(prefix='dconf-keyfile-'))
    body = f"[system/proxy]\nmode='manual'\n\n[system/proxy/http]\nhost='127.0.0.1'\nport={port}\n\n[system/proxy/https]\nhost='127.0.0.1'\nport={port}\n"
    if where == 'sys':
        d = Path('/etc/dconf/db/local.d')
        d.mkdir(parents=True, exist_ok=True)
        (d / '00-proxy').write_text(body)
        Path('/etc/dconf/profile').mkdir(parents=True, exist_ok=True)
        Path('/etc/dconf/profile/user').write_text('user-db:user\nsystem-db:local\n')
        c = run_cmd(['dconf', 'update'])
    else:
        (kdir / 'proxy.key').write_text(body)
        (db_home / '.config' / 'dconf').mkdir(parents=True, exist_ok=True)
        db = db_home / '.config' / 'dconf' / 'user'
        c = run_cmd(['dconf', 'compile', str(db), str(kdir)])
        for d in (db_home / '.config', db_home / '.config' / 'dconf'):
            os.chown(d, account.pw_uid, account.pw_gid)
        os.chown(db, account.pw_uid, account.pw_gid)
    # libproxy 0.5 chooses its configuration backend from the desktop environment: a GNOME session always has XDG_CURRENT_DESKTOP=GNOME (the first baseline,
    # `baseline-a2a001ac`, ran without it: gsettings said 'manual' while `proxy` printed direct://)
    def host_view(home):
        cenv = dict(os.environ, HOME=str(home), XDG_CURRENT_DESKTOP='GNOME')
        p = as_user(['proxy', f'https://{WV_HOST}/'], cenv, timeout=30)
        g = as_user(['gsettings', 'get', 'org.gnome.system.proxy', 'mode'], cenv, timeout=20)
        return {'home': str(home), 'proxyCli': p.stdout.strip(), 'proxyCliRc': p.returncode, 'proxyCliErr': p.stderr.strip()[-200:], 'gsettingsMode': g.stdout.strip(), 'gsettingsErr': g.stderr.strip()[-200:]}
    return {'where': where, 'dbHome': str(db_home), 'guiHome': str(gui_home), 'expected': f'http://127.0.0.1:{port}', 'compileRc': c.returncode, 'compileErr': c.stderr[-200:],
            'hostViewWithGuiHome': host_view(gui_home), 'hostViewWithRealHome': host_view(real)}


def clean_dconf(target_app):
    real = Path(pwd.getpwnam(USER).pw_dir)
    for f in (real / '.config/dconf/user', Path(str(target_app) + '.home') / '.config/dconf/user', Path('/etc/dconf/db/local.d/00-proxy'), Path('/etc/dconf/db/local'),
              Path('/etc/dconf/profile/user')):
        f.unlink(missing_ok=True)


def proxy_env(port):
    url = f'http://127.0.0.1:{port}'
    return {'http_proxy': url, 'https_proxy': url, 'all_proxy': url, 'HTTP_PROXY': url, 'HTTPS_PROXY': url, 'ALL_PROXY': url}


def page_probe_script(prefix, report_port, base, daemon_token, gui_pid):
    """What the shared frontend does against the daemon, run by the real WebView: session exchange (POST /auth/connect with the bearer secret), one authenticated data fetch
    (GET /settings), then a WebSocket to /ws?auth=Session <token> (apps/gui/src/lib/daemon-ws.ts puts the token in the query because browsers cannot set the header), a topic
    subscription to topics that answer with a snapshot (`clipboard` only emits when something is copied: a silent topic proves nothing) and the first decoded event frame (`topic:type`, payload dropped). Every step reports to the loopback report channel. The control file of a run therefore contains the throwaway daemon's bearer token;
    that daemon and its data live only in the run's container."""
    return ("(function(){var P=%s,rp=%d,base=%s,bearer=%s;function rep(k,v){try{fetch('http://127.0.0.1:'+rp+'/'+P+k+'?v='+encodeURIComponent(v),{mode:'no-cors'})}catch(e){}}"
            "fetch(base+'/auth/connect',{method:'POST',headers:{'Authorization':'Bearer '+bearer,'Content-Type':'application/json'},body:JSON.stringify({pid:%d,clientType:'gui'})})"
            ".then(function(r){return r.json().then(function(j){return [r.status,j]})})"
            ".then(function(a){if(a[0]!==200||!a[1].data){throw new Error('connect '+a[0])}rep('connect','ok');var st=a[1].data.sessionToken;"
            "return fetch(base+'/settings',{headers:{'Authorization':'Session '+st}}).then(function(r){return r.json().then(function(j){return [r.status,j,st]})})})"
            ".then(function(a){rep('http',JSON.stringify({status:a[0],hasGeneral:!!(a[1].data&&a[1].data.general)}));var st=a[2];"
            "var ws=new WebSocket(base.replace('http://','ws://')+'/ws?auth='+encodeURIComponent('Session '+st));"
            "ws.onopen=function(){rep('wsopen','1');ws.send(JSON.stringify({action:'subscribe',topics:['status','peers','paired-devices'],nonce:Math.random().toString(36).slice(2)}))};"
            "ws.onmessage=function(e){var m;try{m=JSON.parse(e.data)}catch(x){m=null}if(m&&m.type){rep('wsframe',(m.topic||'')+':'+m.type);ws.close()}};ws.onerror=function(){rep('wserr','1')};})"
            ".catch(function(e){rep('err',String(e))})})()") % (json.dumps(prefix), report_port, json.dumps(base), json.dumps(daemon_token), gui_pid)


P8_VARIANTS = {  # name -> (proxy mode, kind); the environment is built by variant_env()
    'env-upper': ('allow', 'env-upper'),            # only HTTP_PROXY/HTTPS_PROXY/ALL_PROXY (upper case)
    'env-conflict': ('allow', 'env-conflict'),      # lower case -> allow proxy, upper case -> a dead port: which one does the resolver follow?
    'env-bypass': ('allow', 'env-bypass'),          # NO_PROXY names the WebView probe host: it must go direct
    'env-recover': ('allow', 'env-recover'),        # P7: proxied, then the proxy goes away (must FAIL, never go direct), then comes back on the same port (proxied again), same GUI process
    'rv-none': (None, 'rv-none'),                   # P6: Engine rendezvous redeem, no proxy variables: the proxy never sees it (control)
    'rv-deny': ('deny', 'rv-deny'),                 # P6: deny-only proxy: the redeem's rendezvous CONNECT must reach the proxy and be refused (never forwarded)
    'rv-bypass': ('deny', 'rv-bypass'),             # P6: NO_PROXY names the rendezvous host: the proxy must not see that CONNECT
    'env-bypass-other': ('allow', 'env-bypass-other'),  # NO_PROXY names an unrelated host: the WebView is still proxied
}
RENDEZVOUS_HOST = 'rendezvous.uniclipboard.app'  # Engine d4dd324a (uc-engine 1.1.0-rc.22, the packaged daemon's lock entry): uc-infra-p2p RENDEZVOUS_BASE_URL
REQUIRED_VARIANTS = {'rv-none': 'direct', 'rv-deny': 'refused', 'rv-bypass': 'refused', 'env-recover': 'proxied', 'env-bypass': 'direct', 'env-bypass-other': 'proxied'}  # the others are recorded observations (precedence is the resolver library's)


def variant_env(kind, port):
    url = f'http://127.0.0.1:{port}'
    dead = f'http://127.0.0.1:{free_port()}'
    if kind == 'rv-none':
        return {}
    if kind == 'rv-deny':
        return proxy_env(port)
    if kind == 'rv-bypass':
        return dict(proxy_env(port), no_proxy=RENDEZVOUS_HOST, NO_PROXY=RENDEZVOUS_HOST)
    if kind == 'env-recover':
        return proxy_env(port)
    if kind == 'env-upper':
        return {'HTTP_PROXY': url, 'HTTPS_PROXY': url, 'ALL_PROXY': url}
    if kind == 'env-conflict':
        return {'http_proxy': url, 'https_proxy': url, 'HTTP_PROXY': dead, 'HTTPS_PROXY': dead}
    if kind == 'env-bypass':
        return dict(proxy_env(port), no_proxy=f'{WV_HOST},user-bypass.test', NO_PROXY=f'{WV_HOST},user-bypass.test')
    if kind == 'env-bypass-other':
        return dict(proxy_env(port), no_proxy='unrelated.test', NO_PROXY='unrelated.test')
    raise KeyError(kind)


def scenarios():
    """name -> (proxy mode allow|deny|dead|None, configuration kind env|gs-user|gs-ph|gs-sys|None)"""
    s = {'none': (None, None)}
    for m in ('allow', 'deny', 'dead'):
        s[f'env-{m}'] = (m, 'env')
    for where in ('user', 'sys', 'ph'):
        for m in ('allow', 'deny', 'dead'):
            s[f'gs-{where}-{m}'] = (m, f'gs-{where}')
    s.update(P8_VARIANTS)
    return s


def read_maps(pid):
    """(status, libraries): status is 'ok' or 'unreadable: <error>'. The shared maps_of() swallows read errors, so it cannot tell an unreadable process from an empty one."""
    libs = set()
    try:
        text = Path(f'/proc/{pid}/maps').read_text()
    except OSError as e:
        return f'unreadable: {e}', libs
    for line in text.splitlines():
        parts = line.split(None, 5)
        if len(parts) == 6 and '.so' in parts[5]:
            libs.add(parts[5].replace(' (deleted)', ''))
    return 'ok', libs


def modules_loaded(pid):
    return sorted({p.rsplit('/', 1)[-1] for p in maps_of(pid) if PROXY_MODULES.search(p.rsplit('/', 1)[-1])})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--nonportable', action='store_true')
    parser.add_argument('--require', action='store_true', help='evaluate the functional requirements (exit status 3 when only they fail)')
    parser.add_argument('--require-env', action='store_true', help='also require the environment-variable scenarios to be honoured by the WebView')
    parser.add_argument('--scenarios', default='')
    args = parser.parse_args()
    table = scenarios()
    chosen = [s for s in args.scenarios.split(',') if s] or [n for n in table if n not in P8_VARIANTS]  # P8 variants run only when named
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    home = Path(account.pw_dir)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-proxy-'))
    os.chown(sandbox, account.pw_uid, account.pw_gid)
    runtime_dir, install = sandbox / 'run', sandbox / 'install'
    for d in (runtime_dir, install) + ((BUS_DIR,) if args.nonportable else ()):
        d.mkdir(exist_ok=True)
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    target_app = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, target_app)
    os.chown(target_app, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=account.pw_dir, XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
               XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE,
               USER=USER, LOGNAME=USER)
    if args.nonportable:
        env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={BUS_DIR}/bus'
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME',
                *(() if args.nonportable else ('DBUS_SESSION_BUS_ADDRESS',)), 'UC_E2E_BUS', 'SSL_CERT_FILE', 'SSL_CERT_DIR', 'GIO_USE_TLS', 'GIO_MODULE_DIR', 'GIO_EXTRA_MODULES',
                'XDG_CURRENT_DESKTOP', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY'):
        env.pop(key, None)
    run = UserRun(out, target_app, env)
    r = run.results
    osrel = dict(l.split('=', 1) for l in Path('/etc/os-release').read_text().splitlines() if '=' in l)
    r.update({'mode': 'system-proxy-' + ('nonportable' if args.nonportable else 'portable'), 'appimageSha256': sha256(target_app), 'sandbox': str(sandbox), 'user': USER,
              'distribution': osrel.get('PRETTY_NAME', '').strip('"'), 'kernelMachine': os.uname().machine, 'require': args.require, 'requireEnv': args.require_env,
              'scope': 'container (--internal network), unprivileged user, Xvfb, no GTK/WebKitGTK on the host (host proxy-configuration stack present), '
                       + ('NON-portable real HOME with a user session bus and unlocked Secret Service' if args.nonportable else 'portable mode') + '; no desktop, GPU, Wayland, native amd64'})
    r['requirements'] = []

    def req(name, ok, detail=None):
        r['requirements'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('REQ-PASS ' if ok else 'REQ-FAIL ') + name, flush=True)

    xvfb = start_xvfb(out)
    launches, proxies, servers, bus = [], [], [], None
    root = home if args.nonportable else sandbox  # where daemon.conn appears
    try:
        route = subprocess.run('ip route show default; [ -n "$(ip route show default)" ] || ip route add default dev eth0; ip route show default', shell=True, capture_output=True, text=True)
        r['defaultRoute'] = route.stdout
        run.check('T0 fixture: default route present (the Engine fails its p2p bind without one, 17c11 R1); the network is internal (no route out)', 'default' in route.stdout, r['defaultRoute'])
        addr = run_cmd(['sh', '-c', "ip -4 -o addr show eth0 | awk '{print $4}' | cut -d/ -f1"]).stdout.strip()
        run.check('T0 fixture: the container has a non-loopback address for the target', bool(addr) and not addr.startswith('127.'), addr)
        with open('/etc/hosts', 'a') as f:
            f.write(''.join(f'{addr} {h}\n' for h in HOSTS))
        r['hosts'] = Path('/etc/hosts').read_text()
        material = make_target_cert(sandbox)
        r['trustInstall'] = install_trust(material['ca'], 'proxy-target')
        run.check('T0 the target CA is installed through the container\'s own trust mechanism', r['trustInstall']['rc'] == 0, r['trustInstall'])
        target = Target(material, addr)
        servers.append(target)
        reports = Reports()
        loop_url = f'http://127.0.0.1:{reports.port}/loopctl'
        r['hostGioModules'] = sorted(p.name for p in Path('/usr/lib').glob('*/gio/modules/*.so'))
        if args.nonportable:
            bus = subprocess.Popen(['sh', '/work/apps/gui-go/e2e/linux/keyring_service.sh'], env=dict(env, PATH=os.environ['PATH']), user=USER, group=USER, extra_groups=[],
                                   stdout=(out / 'keyring-service.log').open('w'), stderr=subprocess.STDOUT)
            for _ in range(150):
                if (BUS_DIR / 'ready').exists():
                    break
                time.sleep(.2)
            run.check('T1 user session bus and unlocked Secret Service are ready (store and lookup through the Secret Service API)', (BUS_DIR / 'ready').exists(), None)
            if not (BUS_DIR / 'ready').exists():
                raise StopScenario()
            (BUS_DIR / 'bus').chmod(0o777)
        else:
            made = as_user([str(target_app), '--appimage-portable-home'], run.env, timeout=60)
            run.check('T1 portable home created by the AppImage runtime', made.returncode == 0 and Path(str(target_app) + '.home').is_dir(), {'rc': made.returncode, 'err': made.stderr[-300:]})
        r['scenarios'] = {}
        old_pid = None
        for name in chosen:
            mode, kind = table[name]
            sc = r['scenarios'][name] = {'mode': mode, 'kind': kind}
            where = kind.split('-', 1)[1] if kind and kind.startswith('gs-') else None
            if where == 'ph' and args.nonportable:
                sc['skipped'] = 'the portable-HOME causal control only exists in portable mode'
                continue
            proxy, penv, curl_env = None, {}, {}
            if mode in ('allow', 'deny'):
                proxy = Proxy(name, mode)
                proxies.append(proxy)
                port = proxy.port
            else:
                port = free_port() if mode == 'dead' else None  # dead: nothing listens
            if kind == 'env':
                penv = proxy_env(port)
                curl_env = dict(penv)
            elif name in P8_VARIANTS:
                penv = variant_env(kind, port)
                curl_env = {k: v for k, v in penv.items() if k.islower()} or dict(penv)
            elif where:
                sc['dconf'] = write_dconf_proxy(port, where, target_app, args.nonportable)
                penv = {'XDG_CURRENT_DESKTOP': 'GNOME'}  # a GNOME session; NO proxy variable
                cli_out = sc['dconf']['hostViewWithRealHome']['proxyCli'] if where == 'user' else sc['dconf']['hostViewWithGuiHome']['proxyCli']
                curl_env = {'https_proxy': cli_out, 'http_proxy': cli_out} if cli_out.startswith('http') else {}  # curl cannot read gsettings: it gets the host libproxy CLI's answer
            sc['proxyPort'], sc['guiEnvironment'] = port, penv
            gui = run.launch(f'gui-{name}', extra_env=penv)
            launches.append(gui)
            conn_path, conn = wait_new_daemon(root, old_pid) if old_pid else wait_daemon(root)
            run.check(f'[{name}] the real bundled daemon started', conn is not None, str(conn_path))
            if conn is None:
                raise StopScenario()
            old_pid = conn['pid']
            gui.step('bootstrapped', 90)
            state = wait_panel_ready(gui, name, 90)
            run.check(f'[{name}] the real WebView loaded the frontend (quick panel page ready)', state.get('panelReady') is True, state)
            time.sleep(3)  # let the page open its daemon connections
            if args.nonportable:  # 17c11 G7: the host has GTK in this image; the package must still map its own
                # The WebKit network process is found through the process table (exe below THIS GUI's mount), not through its sockets: with loopback traffic proxied it holds none
                # (stage 2, dev: pid -1 and empty path lists were read as "violations"). A process that is missing or whose maps are unreadable is UNVERIFIED, which is neither a
                # pass nor evidence of host contamination.
                gui_exe = procs().get(gui.proc.pid, ('', ''))[0]
                mount = gui_exe.split('/usr/bin/')[0] if '/usr/bin/' in gui_exe else None
                net_pids = [pid for pid, (exe, comm) in procs().items() if mount and exe.startswith(mount) and exe.endswith('/WebKitNetworkProcess')]
                want = {'gui': ([gui.proc.pid], ('libgtk-3', 'libwebkit2gtk-4.1', 'libglib-2.0', 'libgio-2.0')), 'webkitNetwork': (net_pids, ('libglib-2.0', 'libgio-2.0', 'libsoup-3.0'))}
                bad, unverified = {}, []
                for who, (pids, libs) in want.items():
                    if not pids:
                        unverified.append(f'{who}: process not found in the process table')
                        continue
                    status, mapped = read_maps(pids[0])
                    if status != 'ok':
                        unverified.append(f'{who} pid {pids[0]}: {status}')
                        continue
                    for lib in libs:
                        paths = sorted(p for p in mapped if p.rsplit('/', 1)[-1].startswith(lib))
                        if not paths:
                            unverified.append(f'{who} pid {pids[0]}: {lib} is not mapped at all')
                        elif any(not p.startswith('/tmp/.mount_') for p in paths):
                            bad[f'{who}:{lib}'] = paths
                sc['mappedFromMount'] = {'processes': {'guiExe': gui_exe, 'mount': mount, 'webkitNetworkPids': net_pids}, 'violations': bad, 'unverified': unverified}
                run.check(f'[{name}] G7 the GUI and WebKitNetworkProcess map GTK/WebKitGTK/GLib/GIO/libsoup only from the AppImage mount (host GTK present): no violation and nothing unverified',
                          not bad and not unverified, {'violations': bad, 'unverified': unverified})
            daemon_port = conn['port']
            socks = sockets_of(('WebKit', 'uniclipboard', 'uniclipd'))
            sc['sockets'], sc['daemonPort'] = socks, daemon_port
            web_to_daemon = [s for s in socks if s['proc'].startswith('WebKitNetwork') and s['peer'].endswith(f':{daemon_port}')]
            go_to_daemon = [s for s in socks if s['proc'] == 'uniclipboard' and s['peer'].endswith(f':{daemon_port}')]
            sc['owners'] = {'webkitNetworkProcessToDaemon': len(web_to_daemon), 'goHostToDaemon': len(go_to_daemon)}
            run.check(f'[{name}] P1 WebKitNetworkProcess (the WebView itself, not the Go host) holds established loopback TCP connections to the daemon (HTTP vs WebSocket is not distinguished by sockets)',
                      len(web_to_daemon) >= 1, web_to_daemon)
            web_to_proxy = [s for s in socks if s['proc'].startswith('WebKit') and port and s['peer'].endswith(f':{port}')]
            run.check(f'[{name}] P1 no WebKit process holds a connection to the proxy port while the page is idle (no loopback traffic is sent to the proxy)', not web_to_proxy, web_to_proxy)
            sc['daemonProxyEnvironment'] = {k: v for k, v in environ_of(conn['pid']).items() if k.lower() in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy')}
            sc['guiEnvironmentSeen'] = {k: v for k, v in environ_of(gui.proc.pid).items() if k.lower() in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy', 'home', 'xdg_current_desktop')}
            sc['gioModuleDir'] = environ_of(gui.proc.pid).get('GIO_MODULE_DIR')
            # the page's own HTTP fetch and WebSocket against the daemon, under this proxy configuration (the WebView's network stack, not just its sockets)
            base_url, pfx = f'http://{conn["host"]}:{daemon_port}', f'pp-{name}-'
            gui.ctl(f'panel-js pagepp-{name} {page_probe_script(pfx, reports.port, base_url, conn["token"], gui.proc.pid)}', f'panel-js-pagepp-{name}')
            got = {k: reports.wait(pfx + k, 25) for k in ('connect', 'http', 'wsopen', 'wsframe')}
            time.sleep(1)
            page = {k: (v or {}).get('value') for k, v in got.items()}
            page['err'], page['wserr'] = (reports.wait(pfx + 'err', .1) or {}).get('value'), (reports.wait(pfx + 'wserr', .1) or {}).get('value')
            try:
                h = json.loads(page['http'] or '{}')
                page_http_ok = h.get('status') == 200 and h.get('hasGeneral') is True
            except ValueError:
                page_http_ok = False
            page_ws_ok = bool(page['wsopen']) and bool(page['wsframe'])
            sc['pageProbe'] = {'report': page, 'httpOk': page_http_ok, 'wsOk': page_ws_ok}
            run.check(f'[{name}] P1 the real WebView itself completes the daemon session exchange, an authenticated HTTP fetch (GET /settings), opens the daemon WebSocket and receives an event frame', page_http_ok and page_ws_ok, page)
            # P3: WebView first (own hostname, own log window), then curl (own hostname, own window)
            nonce_w, nonce_c = secrets.token_hex(6), secrets.token_hex(6)
            url_w, url_c = f'https://{WV_HOST}/webview-{nonce_w}', f'https://{CURL_HOST}/curl-{nonce_c}'
            n0 = len(proxy.lines()) if proxy else 0
            gui.ctl(f'panel-js ext-{name} {reports.script("ext-" + name, url_w)}', f'panel-js-ext-{name}')
            ev = reports.wait(f'ext-{name}-ok', 40) or reports.wait(f'ext-{name}-err', 25)
            time.sleep(1)
            n1 = len(proxy.lines()) if proxy else 0
            win_w = proxy.lines()[n0:n1] if proxy else []
            ctl = curl_as_user(dict(run.env, **curl_env), url_c)
            time.sleep(1)
            n2 = len(proxy.lines()) if proxy else 0
            win_c = proxy.lines()[n1:n2] if proxy else []
            loop = curl_as_user(dict(run.env, **curl_env), loop_url)
            time.sleep(1)
            sc['webview'] = {'report': ev, 'window': win_w, **classify(WV_HOST, nonce_w, win_w, target)}
            sc['curlControl'] = {'result': ctl, 'window': win_c, **classify(CURL_HOST, nonce_c, win_c, target)}
            sc['curlLoopbackControl'] = loop
            sc['proxyModulesLoadedByWebKitNetworkProcess'] = {str(s['pid']): modules_loaded(s['pid']) for s in {x['pid']: x for x in web_to_daemon}.values()}
            sc['proxyModulesLoadedByGui'] = modules_loaded(gui.proc.pid)
            leaked = []
            if proxy:
                reqs = request_targets(proxy.lines())
                sc['proxyRequestTargets'] = [f'{m} {t}' for m, t in reqs]
                loop_reqs = [t for m, t in reqs if LOOPBACK.search(t)]
                sc['proxyLoopbackTargets'] = loop_reqs
                sc['proxyEngineTargets'] = sorted({t for m, t in reqs if not LOOPBACK.search(t) and not any(h in t for h in (WV_HOST, CURL_HOST))})
                run.check(f'[{name}] control: curl (same configuration, its own hostname) was named by the proxy: the proxy/log chain is valid',
                          sc['curlControl']['route'] in ('proxied', 'refused'), sc['curlControl'])
                run.check(f'[{name}] control: curl targeting the loopback report port with the same configuration IS in the proxy log (detection power for the loopback claim)',
                          any(f':{reports.port}/loopctl' in t for t in loop_reqs), {'curl': loop, 'targets': loop_reqs})
                leaked = [t for t in loop_reqs if f':{daemon_port}' in t or (f':{reports.port}' in t and '/loopctl' not in t)]
                run.check(f'[{name}] P1 the proxy log names no loopback target of the product (not the daemon port, not the page\'s report channel)', not leaked, leaked)
            elif mode == 'dead':
                run.check(f'[{name}] control: curl honours the dead proxy configuration (fails to connect, the target saw nothing)', ctl['rc'] != 0 and sc['curlControl']['targetSaw'] == 0, sc['curlControl'])
            else:
                run.check(f'[{name}] control: curl reaches the target directly without a proxy', sc['curlControl']['targetSaw'] == 1, sc['curlControl'])
            sc['webviewRoute'] = sc['webview']['route']
            # ---- functional requirements (evaluated only with --require; recorded either way)
            visible = not (where == 'user' and not args.nonportable)  # portable HOME hides the real user's dconf (F7): an observation there, not a requirement
            sc['configurationVisibleToGui'] = visible
            applies = kind is not None and visible and (not kind.startswith('env') or args.require_env) and (name not in P8_VARIANTS or name in REQUIRED_VARIANTS)
            sc['requirementApplies'] = applies
            if applies:
                expected = REQUIRED_VARIANTS.get(name) or {'allow': 'proxied', 'deny': 'refused', 'dead': 'failed'}[mode]
                req(f'[{name}] REQUIRE the real WebView request is {expected} (route observed: {sc["webviewRoute"]})', sc['webviewRoute'] == expected, sc['webview'])
                if name in P8_VARIANTS:
                    # /proc/<gui>/environ is the environment the GUI was EXEC'd with: the Go init() changes the runtime's copy, which only a child (the daemon) inherits
                    seen = sc['daemonProxyEnvironment']
                    user_np = [e for e in penv.get('NO_PROXY', '').split(',') if e]
                    got = seen.get('NO_PROXY', '') + ',' + seen.get('no_proxy', '')
                    req(f'[{name}] REQUIRE the environment the GUI hands to its child (the daemon) keeps the user NO_PROXY entries and gains the loopback names', all(x in got for x in user_np + ['127.0.0.1', 'localhost', '::1']), seen)
                if mode in ('deny', 'dead') and name not in P8_VARIANTS:
                    req(f'[{name}] REQUIRE no silent direct escape: the target saw no request from the WebView', sc['webview']['targetSaw'] == 0, sc['webview'])
                req(f'[{name}] REQUIRE the local daemon stays usable: the WebView holds loopback connections to the daemon, the proxy log names no loopback target of the product, and the page itself '
                    f'fetched the daemon over HTTP and received a WebSocket frame', len(web_to_daemon) >= 1 and not leaked and page_http_ok and page_ws_ok,
                    {'webkitToDaemon': len(web_to_daemon), 'leaked': leaked, 'pageProbe': sc['pageProbe']})
            if name == 'env-recover' and proxy:
                def wv_phase(tag):
                    nonce = secrets.token_hex(6)
                    n_a = len(proxy.lines())
                    gui.ctl(f'panel-js ext-{name}-{tag} {reports.script(f"ext-{name}-{tag}", f"https://{WV_HOST}/webview-{tag}-{nonce}")}', f'panel-js-ext-{name}-{tag}')
                    ev_ = reports.wait(f'ext-{name}-{tag}-ok', 40) or reports.wait(f'ext-{name}-{tag}-err', 25)
                    time.sleep(1)
                    return {'report': ev_, **classify(WV_HOST, nonce, proxy.lines()[n_a:], target)}
                proxy.outage()
                out_phase = wv_phase('outage')
                proxy.resume()
                time.sleep(1)
                back_phase = wv_phase('recovered')
                pfx2 = f'pp-{name}-after-'
                gui.ctl(f'panel-js pagepp-{name}-after {page_probe_script(pfx2, reports.port, base_url, conn["token"], gui.proc.pid)}', f'panel-js-pagepp-{name}-after')
                got2 = {k: reports.wait(pfx2 + k, 25) for k in ('connect', 'http', 'wsopen', 'wsframe')}
                sc['p7'] = {'outage': out_phase, 'recovered': back_phase, 'guiPidStable': pid_alive(gui.proc.pid), 'pageAfter': {k: (v or {}).get('value') for k, v in got2.items()}}
                run.check(f'[{name}] P7 phases recorded (outage: {out_phase["route"]}, recovered: {back_phase["route"]})', True, sc['p7'])
                if args.require and args.require_env:
                    req(f'[{name}] REQUIRE while the proxy is down the WebView request FAILS and never reaches the target directly', out_phase['route'] == 'failed' and out_phase['targetSaw'] == 0, out_phase)
                    req(f'[{name}] REQUIRE after the proxy is back on the same port the same GUI process is proxied again', back_phase['route'] == 'proxied', back_phase)
                    req(f'[{name}] REQUIRE the local daemon is still usable after the outage (page HTTP fetch and WebSocket frame)', bool(sc['p7']['pageAfter']['http']) and bool(sc['p7']['pageAfter']['wsframe']), sc['p7']['pageAfter'])
            if name.startswith('rv-'):
                # P6: ONE real redeem of a synthetic invalid invitation against the isolated throwaway daemon (its temp profile). The deny-only proxy records the CONNECT and refuses it.
                import urllib.request, urllib.error
                api = f'http://{conn["host"]}:{daemon_port}'
                direct_opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # the runner talks to the daemon directly
                def call(path, body, auth):
                    rq = urllib.request.Request(api + path, data=json.dumps(body).encode(), method='POST', headers={'Authorization': auth, 'Content-Type': 'application/json'})
                    try:
                        with direct_opener.open(rq, timeout=90) as rs:
                            return rs.status, json.loads(rs.read() or b'{}')
                    except urllib.error.HTTPError as e:
                        return e.code, json.loads(e.read() or b'{}')
                st, body = call('/auth/connect', {'pid': os.getpid(), 'clientType': 'gui'}, 'Bearer ' + conn['token'])
                session = ((body or {}).get('data') or {}).get('sessionToken')
                n_before = len(proxy.lines()) if proxy else 0
                t0 = time.monotonic()
                code = 'uc17c12-synthetic-' + secrets.token_hex(4)
                rst, rbody = call('/v2/setup/redeem', {'code': code, 'passphrase': 'synthetic-' + secrets.token_hex(4)}, 'Session ' + (session or ''))
                elapsed = round(time.monotonic() - t0, 2)
                time.sleep(2)
                win = proxy.lines()[n_before:] if proxy else []
                named = [l for l in win if REQ.search(l) and RENDEZVOUS_HOST in REQ.search(l).group(2)]
                refusals = [l for l in win if 'refused' in l.lower() and RENDEZVOUS_HOST in l]
                sc['p6'] = {'sessionStatus': st, 'redeemStatus': rst, 'redeemBody': {k: v for k, v in (rbody or {}).items() if k != 'data'}, 'elapsedSeconds': elapsed,
                            'proxyWindowNamingRendezvous': named, 'proxyRefusals': refusals, 'proxyWindow': win, 'forwardedToTarget': len(target.requests)}
                run.check(f'[{name}] P6 the synthetic redeem was answered with an error status (never a success)', rst >= 400, sc['p6'])
                if args.require and args.require_env:
                    if name == 'rv-deny':
                        req(f'[{name}] REQUIRE the daemon\'s rendezvous request went through the proxy (CONNECT {RENDEZVOUS_HOST}) and was refused; nothing was forwarded', bool(named) and bool(refusals), sc['p6'])
                    elif name == 'rv-bypass':
                        req(f'[{name}] REQUIRE NO_PROXY naming the rendezvous host keeps the daemon\'s request off the proxy', not named, sc['p6'])
                    else:
                        req(f'[{name}] REQUIRE without proxy variables the redeem is an error and no proxy is involved', rst >= 400 and not named, sc['p6'])
            stop(gui, conn)
            launches.clear()
            if where:
                clean_dconf(target_app)
            if proxy:
                proxy.stop()
    except StopScenario:
        pass
    except Exception as e:
        r['error'] = repr(e)
        import traceback
        r['traceback'] = traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for conn in root.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, 15)
            except (OSError, ValueError, KeyError):
                pass
        if bus:
            bus.terminate()
        for p in proxies:
            p.stop()
            for f in ('tinyproxy.log', 'tinyproxy.conf', 'stdout.log'):
                try:
                    shutil.copy2(p.dir / f, out / f'proxy-{p.tag}-{f}')
                except OSError:
                    pass
        time.sleep(1)
        xvfb.terminate()
        r['targetRequests'] = servers[0].requests if servers else []
        r['targetHandshakeFailures'] = servers[0].handshake_failures if servers else []
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        r['functionalPassed'] = (bool(r['requirements']) and all(c['ok'] for c in r['requirements'])) if args.require else None
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
        for f in out.glob('*.control'):  # the page probe handed the throwaway daemon's bearer secret to the WebView: do not keep it in the shared artifacts
            try:
                f.write_text(re.sub(r'bearer="[^"]*"', 'bearer="<redacted>"', f.read_text(errors='replace')))
            except OSError:
                pass
    print(json.dumps({'passed': r['passed'], 'functionalPassed': r['functionalPassed'], 'mode': r['mode']}))
    sys.exit(0 if r['passed'] and (not args.require or r['functionalPassed']) else (3 if r['passed'] else 1))


if __name__ == '__main__':
    main()
