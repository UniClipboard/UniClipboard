// Stage dlopen-only libraries explicitly; ELF dependency scanning cannot find them.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

if ((process.env.TAURI_ENV_PLATFORM ?? process.platform) === 'linux') {
  const arch = process.env.TAURI_ENV_ARCH ?? process.arch
  const machine = { x86_64: 62, x64: 62, aarch64: 183, arm64: 183 }[arch]
  if (!machine) throw new Error(`Unsupported Linux bundle architecture: ${arch}`)
  const soname = 'libgtk-layer-shell.so.0'
  const cache = execFileSync('/sbin/ldconfig', ['-p'], { encoding: 'utf8' })
  const candidates = cache.split('\n').flatMap(line => {
    const match = line.trim().match(/^(\S+)\s+.*=>\s+(\S+)$/)
    return match?.[1] === soname ? [match[2]] : []
  })
  const source = candidates.find(path => {
    const elf = readFileSync(path)
    return (
      elf.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) &&
      elf[4] === 2 &&
      elf[5] === 1 &&
      elf.readUInt16LE(18) === machine
    )
  })
  if (!source) {
    throw new Error(
      `Install GTK3 Layer Shell for ${arch} before bundling (libgtk-layer-shell0 on Debian, gtk-layer-shell on Arch/Fedora).`
    )
  }
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const destination = resolve(root, 'src-tauri/binaries/linux', soname)
  mkdirSync(dirname(destination), { recursive: true })
  copyFileSync(source, destination)
  console.log(`Staged ${soname} for ${arch}`)
}
