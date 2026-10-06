#!/usr/bin/env python3
"""WebView HTTPS and runtime-library-origin E2E of the self-contained AppImage (slice 17c7,
docs/architecture/gui-go-linux-appimage-runtime-deps.md).

Runs inside a runtime image WITHOUT GTK/WebKitGTK (Ubuntu: uc-gui-go-linux-runtime:17c4, Fedora: uc-gui-go-linux-runtime-fedora:17c7) as an
unprivileged user, portable mode (no Secret Service), real GUI + real release daemon inside the AppImage.

  linux_appimage_tls_run.py --out DIR --appimage X.AppImage --manifest package-manifest.json [--expect-tls works|absent]

The page under test is the real WebView of the GUI (the quick panel page, driven through the e2e control file's `panel-js`): it does
`fetch()` against two local HTTPS servers, one with a certificate from a CA that was installed into THIS container's own system trust
store (the host's mechanism: update-ca-certificates / update-ca-trust), one from a CA the host does not trust. Nothing disables or
replaces TLS verification, and no CA is inside the AppImage.
  --expect-tls works   the shipped contract: trusted -> body read and request seen by the server; untrusted -> rejected, handshake failure
                       logged by the server, no request line
  --expect-tls absent  failing control: asserts the trusted request is ALSO rejected (the AppImage has no TLS backend), proving the scenario
                       can tell the difference
Library origin: after the requests, /proc/<pid>/maps of the GUI and every WebKit process: each `.so` is inside the AppImage mount or an
explicitly classified host library, and nothing the AppImage ships is loaded from the host.
Not proven: real desktop, GPU, Wayland, system proxies, amd64.
"""
import argparse
import http.server
import json
import os
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
import urllib.parse
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_appimage_portable_run import UserRun, USER, as_user, environ_of, stop, wait_daemon  # noqa: E402
from linux_appimage_run import DISPLAY, PASSPHRASE, maps_of, pid_alive, procs, sha256, start_xvfb, wait_panel_ready  # noqa: E402
import pwd  # noqa: E402

# Libraries the AppImage convention (linuxdeploy's exclude list), the GPU driver stack (libglvnd, Mesa and what Mesa's software rasterizer
# needs) and the C library leave to the host. A host library outside this list FAILS the run so that every new one is a documented decision.
HOST_OK = re.compile(r'''^(ld-linux.*|(libc|libm|libdl|libpthread|librt|libresolv|libutil|libanl|libnsl|libcrypt)(-[0-9.]+)?|libnss_.*|
  libEGL|libGL|libGLX|libGLdispatch|libGLESv1_CM|libGLESv2|libOpenGL|libEGL_mesa|libGLX_mesa|libgallium.*|libglapi|libdrm.*|libgbm|
  libwayland-client|libX11|libX11-xcb|libxcb.*|libXau|libXdmcp|libXext|libXfixes|libXrender|libxshmfence|libXxf86vm|libxcb-.*|
  libharfbuzz|libfreetype|libfontconfig|libfribidi|libexpat|libstdc\+\+|libgcc_s(-[0-9A-Za-z.-]+)?|libgmp|libgpg-error|libcom_err|libz|libzstd|
  libLLVM.*|libsensors|libedit|libelf|libffi|libxml2|libbsd|libmd|libtinfo|libncursesw|libgraphite2|libpcre2-8|libbrotli.*|
  libdbus-1|libpng16|libbz2|liblzma|libuuid|libblkid|libmount|libselinux|libcap|libpciaccess|libvulkan.*|libSPIRV.*|libunwind|libicu.*|libXi|libXcursor|libudev|libsystemd|libgcrypt|libdw|libdebuginfod|libacl|libattr|libxxhash)$''', re.X)
HOST_PATH_OK = re.compile(r'/(dri|gbm|vdpau|gallium-pipe)/')


class StopScenario(Exception):
    """A prerequisite failed (already recorded as a failed check): stop the scenario but still write the evidence and exit non-zero. A bare `return` from
    main() skipped the final sys.exit and made a failed run exit 0 (found in the 17c7 final run, control-17c6-fedora)."""


def run_cmd(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def make_ca(workdir, name):
    """An isolated CA and a leaf certificate for 127.0.0.1 / localhost signed by it (openssl CLI, throwaway keys)."""
    d = workdir / name
    d.mkdir()
    ca_key, ca_crt, key, csr, crt = (d / x for x in ('ca.key', 'ca.crt', 'leaf.key', 'leaf.csr', 'leaf.crt'))
    steps = [
        ['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(ca_key), '-out', str(ca_crt), '-days', '2', '-subj', f'/CN=uc-17c7-{name}-test-ca',
         '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'],
        ['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key), '-out', str(csr), '-subj', '/CN=127.0.0.1'],
    ]
    for s in steps:
        r = run_cmd(s)
        if r.returncode:
            raise RuntimeError(f'openssl failed: {s[:3]} {r.stderr}')
    ext = d / 'ext.cnf'
    ext.write_text('subjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nkeyUsage=digitalSignature,keyEncipherment\n')
    r = run_cmd(['openssl', 'x509', '-req', '-in', str(csr), '-CA', str(ca_crt), '-CAkey', str(ca_key), '-CAcreateserial', '-out', str(crt), '-days', '2', '-extfile', str(ext)])
    if r.returncode:
        raise RuntimeError('openssl x509 failed: ' + r.stderr)
    return {'ca': ca_crt, 'cert': crt, 'key': key}


def install_trust(ca_crt, label='trusted'):
    """The container's OWN system trust mechanism (the host's, as an administrator would)."""
    if shutil.which('update-ca-certificates'):
        dest = Path(f'/usr/local/share/ca-certificates/uc-17c7-{label}-test-ca.crt')
        shutil.copy2(ca_crt, dest)
        r = run_cmd(['update-ca-certificates'])
        return {'mechanism': 'update-ca-certificates', 'file': str(dest), 'rc': r.returncode, 'out': r.stdout[-300:] + r.stderr[-300:]}
    if shutil.which('update-ca-trust'):
        dest = Path(f'/etc/pki/ca-trust/source/anchors/uc-17c7-{label}-test-ca.crt')
        shutil.copy2(ca_crt, dest)
        r = run_cmd(['update-ca-trust'])
        return {'mechanism': 'update-ca-trust', 'file': str(dest), 'rc': r.returncode, 'out': r.stdout[-300:] + r.stderr[-300:]}
    raise RuntimeError('no system trust tool (update-ca-certificates / update-ca-trust)')


class TlsServer:
    """HTTPS server that records what it SAW: request lines (with Origin and User-Agent) and failed handshakes."""

    def __init__(self, material, token):
        self.token, self.requests, self.handshake_failures = token, [], []
        outer = self
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(str(material['cert']), str(material['key']))

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                outer.requests.append({'path': self.path, 'origin': self.headers.get('Origin'), 'userAgent': self.headers.get('User-Agent')})
                body = outer.token.encode()
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

        self.server = Server(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def url(self, nonce):
        return f'https://127.0.0.1:{self.port}/token?nonce={nonce}'


class Reports:
    """Loopback listener the page script reports its fetch outcome to."""

    def __init__(self):
        self.events = []
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                q = urllib.parse.urlsplit(self.path)
                outer.events.append({'t': time.monotonic(), 'tag': q.path.strip('/'), 'value': urllib.parse.parse_qs(q.query).get('v', [''])[0]})
                self.send_response(204)
                self.send_header('Access-Control-Allow-Origin', '*')
                self.end_headers()

            def log_message(self, *a):
                pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def wait(self, tag, timeout=45):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for e in self.events:
                if e['tag'] == tag:
                    return e
            time.sleep(.2)
        return None

    def script(self, tag, url):
        return ("(function(){var rp=%d;function rep(k,v){try{fetch('http://127.0.0.1:'+rp+'/'+k+'?v='+encodeURIComponent(v),{mode:'no-cors'})}catch(e){}}"
                "fetch('%s',{mode:'cors',cache:'no-store'}).then(function(r){return r.text()}).then(function(t){rep('%s-ok',t)})"
                ".catch(function(e){rep('%s-err',String(e))})})()") % (self.port, url, tag, tag)


_SONAMES = {}


def elf_soname(path):
    """DT_SONAME through `readelf -d` (cached per path). A host library SHADOWS a shipped one only when both answer to the same soname, so a file
    whose soname cannot be read is NOT waved through: ('error', reason) makes the caller report a violation. ('none', basename) is a readelf success
    for an object that has no SONAME (the dynamic loader itself)."""
    if path not in _SONAMES:
        r = run_cmd(['readelf', '-d', '-W', path])
        if r.returncode != 0:
            _SONAMES[path] = ('error', (r.stderr or r.stdout).strip()[:200] or f'readelf rc={r.returncode}')
        else:
            m = re.search(r'\(SONAME\)\s+Library soname: \[(.+?)\]', r.stdout)
            _SONAMES[path] = ('soname', m.group(1)) if m else ('none', path.rsplit('/', 1)[-1])
    return _SONAMES[path]


def wait_new_daemon(root, old_pid, timeout=90):
    """A daemon.conn whose pid is alive AND differs from the previous run's: right after a stop the old daemon may still answer for a moment."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        path, c = wait_daemon(root, 5)
        if c is not None and c['pid'] != old_pid:
            return path, c
        time.sleep(.5)
    return None, None


def classify_maps(pid, mount, shipped):
    """Every mapped .so of a process: inside the mount / host library (classified) / violation."""
    rows = {'mount': [], 'host': [], 'violations': []}
    for path in sorted(maps_of(pid)):
        base = path.rsplit('/', 1)[-1]
        stem = re.split(r'\.so', base)[0]
        if path.startswith(mount + '/') or path.startswith(mount):
            rows['mount'].append(path)
        elif elf_soname(path)[0] == 'error':
            rows['violations'].append({'path': path, 'why': 'soname unreadable, cannot classify: ' + elf_soname(path)[1]})
        elif base in shipped or elf_soname(path)[1] in shipped:
            rows['violations'].append({'path': path, 'why': f'a library the AppImage ships (soname {elf_soname(path)[1]}) is loaded from the host'})
        elif HOST_OK.match(stem) or HOST_PATH_OK.search(path):
            rows['host'].append(path)
            rows.setdefault('hostSonames', {})[path] = elf_soname(path)[1]
        else:
            rows['violations'].append({'path': path, 'why': 'unclassified host library'})
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--expect-tls', choices=('works', 'absent'), default='works')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-tls-'))
    os.chown(sandbox, account.pw_uid, account.pw_gid)
    runtime_dir = sandbox / 'run'
    runtime_dir.mkdir()
    os.chown(runtime_dir, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    install = sandbox / 'install'
    install.mkdir()
    os.chown(install, account.pw_uid, account.pw_gid)
    target = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, target)
    os.chown(target, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=pwd.getpwnam(USER).pw_dir, XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
               XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE,
               USER=USER, LOGNAME=USER)
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME', 'DBUS_SESSION_BUS_ADDRESS',
                'UC_E2E_BUS', 'SSL_CERT_FILE', 'SSL_CERT_DIR', 'GIO_USE_TLS', 'GIO_MODULE_DIR', 'GIO_EXTRA_MODULES'):
        env.pop(key, None)
    run = UserRun(out, target, env)
    r = run.results
    osrel = dict(l.split('=', 1) for l in Path('/etc/os-release').read_text().splitlines() if '=' in l)
    r.update({'mode': f'tls-{args.expect_tls}', 'appimageSha256': sha256(target), 'sandbox': str(sandbox), 'user': USER,
              'distribution': osrel.get('PRETTY_NAME', '').strip('"'), 'kernelMachine': os.uname().machine,
              'scope': 'container, unprivileged user, Xvfb, no GTK/WebKitGTK on the host, portable mode; no desktop, GPU, Wayland or system proxy'})
    xvfb = start_xvfb(out)
    launches = []
    servers = []
    try:
        loader = run_cmd(['ldconfig', '-p']).stdout
        bad = [l for l in loader.splitlines() if 'libwebkit2gtk' in l or 'libgtk-3' in l or 'libsoup' in l]
        run.check('T0 clean host: no libwebkit2gtk / libgtk-3 / libsoup in the loader cache', not bad, bad)
        r['hostGLib'] = [l.strip() for l in loader.splitlines() if 'libglib-2.0' in l or 'libgio-2.0' in l]
        hostgio = sorted(str(p) for pat in ('/usr/lib*/gio/modules/*', '/usr/lib/*/gio/modules/*') for p in Path('/').glob(pat.lstrip('/')))
        r['hostGioModules'] = hostgio

        fixtures = sandbox / 'pki'
        fixtures.mkdir()
        trusted, untrusted = make_ca(fixtures, 'trusted'), make_ca(fixtures, 'untrusted')
        r['trustInstall'] = install_trust(trusted['ca'])
        run.check('T0 the trusted test CA is installed through the container\'s own trust mechanism (not into the AppImage)', r['trustInstall']['rc'] == 0, r['trustInstall'])
        tok_t, tok_u = secrets.token_hex(12), secrets.token_hex(12)
        srv_t, srv_u = TlsServer(trusted, tok_t), TlsServer(untrusted, tok_u)
        servers += [srv_t, srv_u]
        # Control of the control: a client that uses the host's default trust (python/OpenSSL, the host's own store) must accept the trusted
        # server and reject the untrusted one, otherwise the fixtures prove nothing about the AppImage.
        def host_client(port):
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=10) as s, ssl.create_default_context().wrap_socket(s, server_hostname='127.0.0.1') as t:
                    t.sendall(b'GET /host-control HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n')
                    return {'ok': True, 'reply': t.recv(4096).decode(errors='replace')[-80:]}
            except (ssl.SSLError, OSError) as e:
                return {'ok': False, 'error': str(e)[:200]}
        hc_t, hc_u = host_client(srv_t.port), host_client(srv_u.port)
        r['hostClientControl'] = {'trusted': hc_t, 'untrusted': hc_u}
        run.check('T0 fixture control: a client using the HOST trust store accepts the trusted server and rejects the untrusted one', hc_t['ok'] and not hc_u['ok'], r['hostClientControl'])
        srv_t.requests.clear()
        srv_u.requests.clear()
        srv_t.handshake_failures.clear()
        srv_u.handshake_failures.clear()

        home_dir = Path(str(target) + '.home')
        made = as_user([str(target), '--appimage-portable-home'], run.env, timeout=60)
        run.check('T1 portable home created by the AppImage runtime (no Secret Service needed)', made.returncode == 0 and home_dir.is_dir(), {'rc': made.returncode, 'err': made.stderr[-300:]})
        gui = run.launch('gui1')
        launches.append(gui)
        conn_path, conn = wait_daemon(sandbox)
        run.check('T1 the real bundled daemon started and published daemon.conn', conn is not None, str(conn_path))
        if conn is None:
            raise StopScenario()
        gui.step('bootstrapped', 90)
        state = wait_panel_ready(gui, 'tls', 90)
        run.check('T1 the real WebView loaded the frontend (quick panel page ready)', state.get('panelReady') is True, state)
        table = procs()
        gui_exe = table.get(gui.proc.pid, ('', ''))[0]
        mount = gui_exe.split('/usr/bin/')[0] if '/usr/bin/' in gui_exe else None
        run.check('T1 the GUI executes from the AppImage mount', bool(mount) and mount.startswith('/tmp/.mount_'), gui_exe)
        if not mount:
            raise StopScenario()
        shipped = set(as_user(['find', mount, '(', '-name', '*.so', '-o', '-name', '*.so.*', ')'], run.env).stdout.split())
        shipped = {p.rsplit('/', 1)[-1] for p in shipped}
        r['shippedSonames'] = len(shipped)
        gio_dir = as_user(['ls', '-1', f'{mount}/usr/lib/gio/modules'], run.env).stdout.split()
        r['bundledGioModules'] = gio_dir
        gnutls = as_user(['python3', '-c', "import re,sys,glob;p=glob.glob(sys.argv[1]+'/usr/lib/libgnutls.so.30')[0];d=open(p,'rb').read();"
                          "print('\\n'.join(sorted({m.decode() for m in re.findall(rb'(/etc/[A-Za-z0-9_./-]*(?:cert|ca-|trust|pem|crt)[A-Za-z0-9_./-]*|pkcs11:[A-Za-z0-9_=;:%-]+)', d)})))", mount], run.env)
        r['gnutlsCompiledTrustPaths'] = gnutls.stdout.split()
        r['hostTrustFiles'] = {p: Path(p).exists() for p in r['gnutlsCompiledTrustPaths'] if p.startswith('/etc/')}

        reports = Reports()
        nonce_t, nonce_u = secrets.token_hex(8), secrets.token_hex(8)
        r['nonces'] = {'trusted': nonce_t, 'untrusted': nonce_u, 'bodyTokens': {'trusted': tok_t, 'untrusted': tok_u}}
        gui.ctl(f'panel-js tls-trusted {reports.script("trusted", srv_t.url(nonce_t))}', 'panel-js-tls-trusted')
        ev_t = reports.wait('trusted-ok') or reports.wait('trusted-err', 15)
        gui.ctl(f'panel-js tls-untrusted {reports.script("untrusted", srv_u.url(nonce_u))}', 'panel-js-tls-untrusted')
        ev_u = reports.wait('untrusted-ok', 5) or reports.wait('untrusted-err', 40)
        time.sleep(1)
        r['webviewReports'] = {'trusted': ev_t, 'untrusted': ev_u}
        r['serverSaw'] = {'trusted': {'requests': srv_t.requests, 'handshakeFailures': srv_t.handshake_failures},
                          'untrusted': {'requests': srv_u.requests, 'handshakeFailures': srv_u.handshake_failures}}
        guilog = Path(gui.log).read_text(errors='replace')
        r['guiLogTlsLines'] = [l for l in guilog.splitlines() if re.search(r'(?i)tls|ssl|gnutls|glib-networking|certificate', l)][:20]

        if args.expect_tls == 'works':
            run.check('T2 the WebView read the trusted HTTPS server\'s body through the host-trusted CA', ev_t and ev_t['tag'] == 'trusted-ok' and ev_t['value'] == tok_t, ev_t)
            run.check('T2 the trusted server saw exactly that WebView request (WebKit user agent of the Wails page, origin wails://localhost)',
                      len(srv_t.requests) == 1 and srv_t.requests[0]['path'] == f'/token?nonce={nonce_t}' and 'AppleWebKit' in (srv_t.requests[0]['userAgent'] or '')
                      and srv_t.requests[0]['origin'] == 'wails://localhost', srv_t.requests)
        else:
            run.check('T2 CONTROL: without the TLS backend the trusted HTTPS request is rejected too (server saw no request, and no certificate alert: the client never got to verification)',
                      ev_t is not None and ev_t['tag'] == 'trusted-err' and not srv_t.requests and not any('ALERT' in f for f in srv_t.handshake_failures),
                      {'report': ev_t, 'requests': srv_t.requests, 'handshake': srv_t.handshake_failures})
        run.check('T3 the untrusted server is rejected by the WebView: error reported, NO request line reached it',
                  ev_u is not None and ev_u['tag'] == 'untrusted-err' and not srv_u.requests, {'report': ev_u, 'requests': srv_u.requests})
        if args.expect_tls == 'works':
            # The same WebView, the same code path and the same host store accepted the trusted server above; the only difference is the CA. The client's
            # unknown-CA alert is recorded when the server sees it, but a TCP reset can overtake it (dev5 showed only "Connection reset by peer"), so the
            # alert text is evidence, not the assertion: what is asserted is that the handshake never completed (failure on the server) and no request arrived.
            r['untrustedAlertSeen'] = any('UNKNOWN_CA' in f for f in srv_u.handshake_failures)
            run.check('T3 the untrusted server saw the handshake fail on the client side (alert or reset), never a request: the CA is the only difference to the trusted case',
                      len(srv_u.handshake_failures) >= 1 and not srv_u.requests, {'handshakeFailures': srv_u.handshake_failures, 'unknownCaAlertSeen': r['untrustedAlertSeen']})

        # T4: library origin
        pids = {pid: v[0] for pid, v in procs().items() if v[0].startswith(mount)}
        names = {pid: exe.rsplit('/', 1)[-1] for pid, exe in pids.items()}
        r['mountProcesses'] = {str(k): v for k, v in names.items()}
        wanted = {'uniclipboard', 'WebKitWebProcess', 'WebKitNetworkProcess'}
        run.check('T4 GUI, WebKitWebProcess and WebKitNetworkProcess all run from the mount', wanted <= set(names.values()), sorted(set(names.values())))
        maps = {}
        for pid, name in names.items():
            if name in wanted | {'WebKitGPUProcess'}:
                maps[f'{name}:{pid}'] = classify_maps(pid, mount, shipped)
        r['maps'] = maps
        bad_maps = {k: v['violations'] for k, v in maps.items() if v['violations']}
        run.check('T4 every mapped .so is inside the AppImage mount or a classified host library; nothing the AppImage ships is loaded from the host', not bad_maps, bad_maps)
        net = next((v for k, v in maps.items() if k.startswith('WebKitNetworkProcess')), {'mount': []})
        net_mount = [p.rsplit('/', 1)[-1] for p in net['mount']]
        if args.expect_tls == 'works':
            run.check('T4 WebKitNetworkProcess maps libgiognutls.so and libgnutls.so.30 from the mount', 'libgiognutls.so' in net_mount and 'libgnutls.so.30' in net_mount, sorted(net_mount))
        glesv2 = {k: [p for p in v['host'] if 'libGLESv2' in p] for k, v in maps.items()}
        r['libGLESv2Origin'] = glesv2
        run.check('T4 no GPU driver-stack library (libEGL/libGL/libGLX/libOpenGL/libGLES*) is loaded from the mount',
                  not any(re.match(r'lib(EGL|GL|GLX|OpenGL|GLESv1_CM|GLESv2)\.so', p.rsplit('/', 1)[-1]) for v in maps.values() for p in v['mount']), None)
        run.check('T4 the host\'s own GIO module directory is not the one used: GIO_MODULE_DIR of the GUI points into the mount',
                  environ_of(gui.proc.pid).get('GIO_MODULE_DIR', '').startswith(mount), environ_of(gui.proc.pid).get('GIO_MODULE_DIR'))
        stop(gui, conn)
        launches.clear()
        if args.expect_tls == 'works':
            # T5 causal control for T3: the SAME untrusted server and URL shape, but the host now trusts its CA (the container's own mechanism, a second
            # file next to the first). The GUI is restarted because GLib's default TLS database is read once per process. If the earlier rejection was
            # certificate verification, the request now succeeds with a request line at the server; if it was anything else, it still fails.
            r['untrustedCaInstall'] = install_trust(untrusted['ca'], 'untrusted-now-trusted')
            run.check('T5 the previously untrusted CA is installed into the host trust store', r['untrustedCaInstall']['rc'] == 0, r['untrustedCaInstall'])
            before = len(srv_u.requests)
            gui2 = run.launch('gui2')
            launches.append(gui2)
            conn2_path, conn2 = wait_new_daemon(sandbox, conn['pid'])
            if conn2 is not None:
                gui2.step('bootstrapped', 90)
            state2 = wait_panel_ready(gui2, 'tls2', 90) if conn2 else {}
            run.check('T5 second start: daemon and WebView ready again', conn2 is not None and state2.get('panelReady') is True, state2)
            nonce_u2 = secrets.token_hex(8)
            gui2.ctl(f'panel-js tls-untrusted2 {reports.script("untrusted2", srv_u.url(nonce_u2))}', 'panel-js-tls-untrusted2')
            ev_u2 = reports.wait('untrusted2-ok', 45) or reports.wait('untrusted2-err', 5)
            r['webviewReports']['untrustedAfterTrust'] = ev_u2
            run.check('T5 once the host trusts that CA, the same WebView request to the same server succeeds (body read, request line with the new nonce): the rejection was certificate trust',
                      ev_u2 is not None and ev_u2['tag'] == 'untrusted2-ok' and ev_u2['value'] == tok_u and len(srv_u.requests) == before + 1
                      and srv_u.requests[-1]['path'] == f'/token?nonce={nonce_u2}', {'report': ev_u2, 'requests': srv_u.requests})
            stop(gui2, conn2)
            launches.clear()
        r['passed'] = all(c['ok'] for c in r['checks'])
    except StopScenario:
        pass
    except Exception as e:  # keep the evidence of a failed run
        r['error'] = repr(e)
        import traceback
        r['traceback'] = traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        r.setdefault('passed', False) if not r.get('passed') else None
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for conn in sandbox.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, 15)
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(1)
        xvfb.terminate()
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
    print(json.dumps({'passed': r['passed'], 'mode': r['mode']}))
    sys.exit(0 if r['passed'] else 1)


if __name__ == '__main__':
    main()
