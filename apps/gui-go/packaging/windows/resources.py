"""Generate the GUI's Windows resources with the project's pinned Wails CLI."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile

GUI = Path(__file__).resolve().parents[2]


def wails_version():
    return re.search(r'github.com/wailsapp/wails/v3 (v[^\s]+)', (GUI / 'go.mod').read_text()).group(1)


def verify_app_icon(executable):
    """Check the linked PE resources against every image in the canonical ICO."""
    data = Path(executable).read_bytes()
    pe = struct.unpack_from('<I', data, 60)[0]
    optional = pe + 24
    directories = optional + (112 if struct.unpack_from('<H', data, optional)[0] == 0x20b else 96)
    resource_rva, resource_size = struct.unpack_from('<II', data, directories + 16)
    if not resource_rva or not resource_size:
        raise ValueError(f'{executable}: missing Windows icon resources')
    sections = optional + struct.unpack_from('<H', data, pe + 20)[0]
    count = struct.unpack_from('<H', data, pe + 6)[0]

    def offset(rva):
        for i in range(count):
            size, address, raw_size, raw = struct.unpack_from('<IIII', data, sections + i * 40 + 8)
            if address <= rva < address + max(size, raw_size):
                return raw + rva - address
        raise ValueError(f'{executable}: invalid resource address {rva:#x}')

    root = offset(resource_rva)

    def entries(relative):
        named, ids = struct.unpack_from('<HH', data, root + relative + 12)
        return [struct.unpack_from('<II', data, root + relative + 16 + i * 8)
                for i in range(named + ids)]

    types = dict(entries(0))
    if 3 not in types or 14 not in types:
        raise ValueError(f'{executable}: missing RT_ICON or RT_GROUP_ICON')
    images = set()
    for _, names in entries(types[3] & 0x7fffffff):
        for _, leaf in entries(names & 0x7fffffff):
            rva, size = struct.unpack_from('<II', data, root + leaf)
            start = offset(rva)
            images.add(data[start:start + size])
    ico = (GUI / 'icons/icon.ico').read_bytes()
    for i in range(struct.unpack_from('<H', ico, 4)[0]):
        size, start = struct.unpack_from('<II', ico, 6 + i * 16 + 8)
        if ico[start:start + size] not in images:
            raise ValueError(f'{executable}: embedded icon differs from icons/icon.ico')


@contextmanager
def gui_resources(arch, version, identifier, product):
    syso = GUI / f'wails_windows_{arch}.syso'
    if syso.exists():
        raise FileExistsError(f'{syso}: refusing to overwrite existing build resources')
    with tempfile.TemporaryDirectory(prefix='uc-windows-resources-') as tmp:
        work = Path(tmp)
        cli_version = wails_version()
        subprocess.run(['go', 'install', f'github.com/wailsapp/wails/v3/cmd/wails3@{cli_version}'],
                       cwd=work, env=dict(os.environ, GOBIN=str(work)), check=True)
        module = Path(subprocess.check_output(['go', 'list', '-m', '-f', '{{.Dir}}',
                                              'github.com/wailsapp/wails/v3'], cwd=GUI, text=True).strip())
        template = (module / 'internal/commands/updatable_build_assets/windows/wails.exe.manifest.tmpl').read_text()
        manifest = work / 'app.manifest'
        manifest.write_text(template.replace('{{.ProductIdentifier}}', identifier)
                           .replace('{{.ProductVersion}}', version + '.0'))
        info = work / 'info.json'
        info.write_text(json.dumps({'fixed': {'file_version': version}, 'info': {'0000': {
            'ProductVersion': version, 'CompanyName': product, 'FileDescription': product,
            'LegalCopyright': product, 'ProductName': product, 'Comments': ''}}}))
        try:
            subprocess.run([str(work / ('wails3.exe' if os.name == 'nt' else 'wails3')), 'generate', 'syso',
                            '-arch', arch, '-icon', str(GUI / 'icons/icon.ico'), '-manifest', str(manifest),
                            '-info', str(info), '-out', str(syso)], check=True)
            yield
        finally:
            syso.unlink(missing_ok=True)
