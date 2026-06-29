const FILE_TYPE_COLORS: { exts: string[]; color: string }[] = [
  { exts: ['PDF'], color: 'rgb(212,88,82)' },
  { exts: ['DOC', 'DOCX', 'RTF', 'TXT', 'MD', 'PAGES'], color: 'rgb(72,118,196)' },
  { exts: ['XLS', 'XLSX', 'CSV', 'NUMBERS'], color: 'rgb(58,158,108)' },
  { exts: ['PPT', 'PPTX', 'KEY'], color: 'rgb(218,138,72)' },
  { exts: ['ZIP', 'RAR', '7Z', 'GZ', 'TAR'], color: 'rgb(176,142,96)' },
  { exts: ['PNG', 'JPG', 'JPEG', 'GIF', 'SVG', 'WEBP', 'HEIC', 'BMP'], color: 'rgb(150,112,202)' },
  { exts: ['MP4', 'MOV', 'AVI', 'MKV', 'WEBM'], color: 'rgb(92,120,210)' },
  { exts: ['MP3', 'WAV', 'FLAC', 'AAC', 'M4A'], color: 'rgb(202,100,150)' },
  {
    exts: ['JS', 'TS', 'TSX', 'JSX', 'PY', 'RS', 'GO', 'JSON', 'HTML', 'CSS', 'SH'],
    color: 'rgb(110,120,136)',
  },
]

const EXT_COLOR = new Map<string, string>(
  FILE_TYPE_COLORS.flatMap(group => group.exts.map(ext => [ext, group.color] as const))
)

function fileTypeColor(ext: string): string {
  return EXT_COLOR.get(ext.toUpperCase()) ?? 'rgb(140,150,160)'
}

interface FileGlyphProps {
  ext: string
  stacked?: boolean
}

function FileGlyph({ ext, stacked }: FileGlyphProps) {
  const color = fileTypeColor(ext)
  const label = ext.length > 4 ? ext.slice(0, 4) : ext
  return (
    <div className="relative shrink-0">
      {stacked && (
        <div
          aria-hidden
          className="absolute -right-1 -top-1 h-12 w-10 rounded-md bg-muted-foreground/25"
        />
      )}
      <div
        className="relative flex h-12 w-10 items-center justify-center overflow-hidden rounded-md"
        style={{ backgroundColor: color }}
      >
        <div className="absolute right-0 top-0 size-3 rounded-bl-md bg-black/20" />
        <span className="px-0.5 text-[9px] font-bold uppercase tracking-wide text-white">
          {label}
        </span>
      </div>
    </div>
  )
}

export default FileGlyph
