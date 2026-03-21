/** Displays the OS logo based on guest_os string from VmConfig. */

interface Props {
  guestOs: string
  size?: number
  className?: string
}

/** Map guest_os enum values to icon filenames. */
function getIconFile(guestOs: string): string {
  const os = guestOs.toLowerCase()
  if (os.includes('windows') || os === 'win7' || os === 'win10' || os === 'win11' || os === 'winxp') return 'windows.png'
  if (os.includes('linux') || os.includes('ubuntu') || os.includes('debian') || os.includes('fedora')
    || os.includes('centos') || os.includes('arch') || os.includes('manjaro') || os.includes('mint')
    || os.includes('suse') || os.includes('redhat') || os.includes('rhel')) return 'linux.png'
  return 'other.png'
}

/** Background color tint based on OS. */
function getBgClass(guestOs: string): string {
  const os = guestOs.toLowerCase()
  if (os.includes('windows') || os === 'win7' || os === 'win10' || os === 'win11' || os === 'winxp') return 'bg-blue-500/10'
  if (os.includes('linux') || os.includes('ubuntu') || os.includes('debian') || os.includes('fedora')
    || os.includes('centos') || os.includes('arch') || os.includes('manjaro') || os.includes('mint')
    || os.includes('suse') || os.includes('redhat') || os.includes('rhel')) return 'bg-amber-500/10'
  return 'bg-vmm-surface-hover'
}

export default function OsIcon({ guestOs, size = 40, className = '' }: Props) {
  const file = getIconFile(guestOs)
  const bgClass = getBgClass(guestOs)
  const imgSize = Math.round(size * 0.55)

  return (
    <div
      className={`rounded-xl flex items-center justify-center ${bgClass} ${className}`}
      style={{ width: size, height: size }}
    >
      <img
        src={`/icons/os/${file}`}
        alt={guestOs}
        style={{ width: imgSize, height: imgSize }}
        className="object-contain"
      />
    </div>
  )
}
