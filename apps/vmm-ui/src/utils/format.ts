export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`
}

export function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400)
  const h = Math.floor((seconds % 86400) / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  return `${d}d ${h}h ${m}m`
}

export function formatRam(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
  return `${mb} MB`
}

export function guestOsLabel(os: string): string {
  const map: Record<string, string> = {
    win7: 'Windows 7', win8: 'Windows 8', win10: 'Windows 10', win11: 'Windows 11',
    winserver2016: 'WinServer 2016', winserver2019: 'WinServer 2019', winserver2022: 'WinServer 2022',
    ubuntu: 'Ubuntu', debian: 'Debian', fedora: 'Fedora', opensuse: 'openSUSE',
    redhat: 'Red Hat', arch: 'Arch Linux', linux: 'Linux', freebsd: 'FreeBSD',
    dos: 'DOS', other: 'Other',
  }
  return map[os] || os
}
