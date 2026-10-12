/** NSProgressIndicator spinning style: eight spokes stepping round, still under reduced motion. */
export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <svg
      className="mac-spinner"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.4"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M12 3v4" />
      <path d="M18.4 5.6l-2.8 2.8" opacity=".3" />
      <path d="M21 12h-4" opacity=".35" />
      <path d="M18.4 18.4l-2.8-2.8" opacity=".45" />
      <path d="M12 21v-4" opacity=".55" />
      <path d="M5.6 18.4l2.8-2.8" opacity=".65" />
      <path d="M3 12h4" opacity=".75" />
      <path d="M5.6 5.6l2.8 2.8" opacity=".9" />
    </svg>
  )
}
