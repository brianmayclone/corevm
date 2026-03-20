import { useState, useRef } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ArrowLeft, Maximize2, Minimize2 } from 'lucide-react'
import Button from '../components/Button'
import ConsoleCanvas from '../components/ConsoleCanvas'

export default function VmConsole() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [fullscreen, setFullscreen] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)

  const toggleFullscreen = () => {
    if (!document.fullscreenElement) {
      containerRef.current?.requestFullscreen()
      setFullscreen(true)
    } else {
      document.exitFullscreen()
      setFullscreen(false)
    }
  }

  if (!id) return null

  return (
    <div ref={containerRef} className="flex flex-col h-full -m-6 bg-black">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 bg-vmm-sidebar border-b border-vmm-border flex-shrink-0">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" icon={<ArrowLeft size={14} />} onClick={() => navigate(`/vms/${id}`)}>
            Back
          </Button>
        </div>
        <Button variant="ghost" size="icon" onClick={toggleFullscreen}>
          {fullscreen ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </Button>
      </div>

      {/* Console fills remaining space */}
      <div className="flex-1 flex items-center justify-center overflow-hidden p-2">
        <div className="w-full max-h-full" style={{ maxWidth: '100%' }}>
          <ConsoleCanvas vmId={id} captureKeyboard />
        </div>
      </div>
    </div>
  )
}
