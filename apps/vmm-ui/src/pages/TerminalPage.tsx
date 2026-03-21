import Terminal from '../components/Terminal'

export default function TerminalPage() {
  return (
    <div className="h-full flex flex-col">
      <div className="mb-4 sm:mb-6">
        <h1 className="text-xl sm:text-2xl font-bold">Terminal</h1>
        <p className="text-xs sm:text-sm text-gray-400 mt-1">
          Manage VMs and system resources via command line
        </p>
      </div>
      <Terminal className="flex-1 min-h-0" />
    </div>
  )
}
