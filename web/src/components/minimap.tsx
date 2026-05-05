import { Crosshair, Target } from "@phosphor-icons/react"
import { memo, useCallback, useEffect, useRef, useState } from "react"

import { minimapColor } from "@/lib/dashboard"
import type { AiEnemySnapshot, BotTarget, CollectableSnapshot, MinimapSnapshot, PlayerPosition, RemotePlayerSnapshot } from "@/lib/types"
import { getAtlas, peekAtlas, type AtlasBundle } from "@/lib/atlas"

type Props = {
  minimap: MinimapSnapshot | null
  playerPosition: PlayerPosition
  aiEnemies: AiEnemySnapshot[]
  otherPlayers: RemotePlayerSnapshot[]
  currentWorld: string | null
  currentTarget: BotTarget | null
  collectables: CollectableSnapshot[]
  onHoverChange: (value: string) => void
}

const MIN_ZOOM = 0.5
const MAX_ZOOM = 16

function unpackColor(value: number): [number, number, number] {
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255]
}

function blend(
  base: [number, number, number],
  overlay: [number, number, number],
  alpha: number,
): [number, number, number] {
  return [
    Math.round(base[0] * (1 - alpha) + overlay[0] * alpha),
    Math.round(base[1] * (1 - alpha) + overlay[1] * alpha),
    Math.round(base[2] * (1 - alpha) + overlay[2] * alpha),
  ]
}

function rasterize(
  snap: MinimapSnapshot,
  atlasTiles?: Record<string, [number, number]>,
): ImageData {
  const { width, height, foreground_tiles, background_tiles, water_tiles, wiring_tiles } = snap
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const i = y * width + x
      const screenY = height - 1 - y
      const o = (screenY * width + x) * 4
      const bg = background_tiles[i] ?? 0
      const fg = foreground_tiles[i] ?? 0
      const w = water_tiles[i] ?? 0
      const wr = wiring_tiles[i] ?? 0
      let c: [number, number, number] = bg
        ? unpackColor(minimapColor(bg, "background"))
        : [16, 22, 34]
      const fgHasSprite = !!fg && !!atlasTiles && atlasTiles[String(fg)] !== undefined
      if (fg && !fgHasSprite) c = unpackColor(minimapColor(fg, "foreground"))
      if (w) c = blend(c, unpackColor(minimapColor(w, "water")), 0.55)
      if (wr) c = blend(c, unpackColor(minimapColor(wr, "wiring")), 0.45)
      data[o] = c[0]
      data[o + 1] = c[1]
      data[o + 2] = c[2]
      data[o + 3] = 255
    }
  }
  return new ImageData(data, width, height)
}

interface TileInfo {
  mapX: number
  mapY: number
  fg: number
  bg: number
  water: number
  wiring: number
}

interface EntityVisual {
  x: number
  y: number
}

function MinimapPanelImpl({ minimap, playerPosition, aiEnemies, otherPlayers, currentWorld, currentTarget, collectables, onHoverChange }: Props) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const tileLayerRef = useRef<HTMLCanvasElement | null>(null)
  const atlasImgRef = useRef<HTMLImageElement | null>(null)
  const atlasMetaRef = useRef<{ cell: number; tiles: Record<string, [number, number]> } | null>(
    null,
  )
  const [atlasResolved, setAtlasResolved] = useState(() => peekAtlas() !== null)
  const [hover, setHover] = useState<TileInfo | null>(null)
  const [view, setView] = useState({ zoom: 1, panX: 0, panY: 0 })
  const viewRef = useRef(view)
  const layoutRef = useRef({ dx: 0, dy: 0, scale: 1, w: 0, h: 0 })
  const onHoverChangeRef = useRef(onHoverChange)
  onHoverChangeRef.current = onHoverChange

  // Interpolation state
  const visualPlayerRef = useRef<EntityVisual | null>(null)
  const visualEnemiesRef = useRef<Map<number, EntityVisual>>(new Map())
  const visualOthersRef = useRef<Map<string, EntityVisual>>(new Map())
  const lastFrameTimeRef = useRef<number>(performance.now())

  useEffect(() => {
    const cached = peekAtlas()
    if (cached) {
      atlasImgRef.current = cached.image
      atlasMetaRef.current = cached.meta
      setAtlasResolved(true)
      return
    }
    let cancelled = false
    getAtlas().then((bundle: AtlasBundle | null) => {
      if (cancelled) return
      if (bundle) {
        atlasImgRef.current = bundle.image
        atlasMetaRef.current = bundle.meta
      }
      setAtlasResolved(true)
    })
    return () => {
      cancelled = true
    }
  }, [])

  const minimapRef = useRef<MinimapSnapshot | null>(minimap)
  minimapRef.current = minimap

  const lastWorldRef = useRef<string | null>(null)
  useEffect(() => {
    if (lastWorldRef.current !== currentWorld) {
      lastWorldRef.current = currentWorld
      setView({ zoom: 1, panX: 0, panY: 0 })
      visualPlayerRef.current = null
      visualEnemiesRef.current.clear()
      visualOthersRef.current.clear()
    }
  }, [currentWorld])

  useEffect(() => {
    viewRef.current = view
  }, [view])

  useEffect(() => {
    if (!minimap || !atlasResolved) {
      tileLayerRef.current = null
      return
    }
    const off = document.createElement("canvas")
    off.width = minimap.width
    off.height = minimap.height
    const ctx = off.getContext("2d")
    if (!ctx) return
    ctx.putImageData(rasterize(minimap, atlasMetaRef.current?.tiles), 0, 0)
    tileLayerRef.current = off
  }, [minimap, atlasResolved])

  const drawScene = useCallback(() => {
    const canvas = canvasRef.current
    const layer = tileLayerRef.current
    if (!canvas || !minimap) return
    const ctx = canvas.getContext("2d")
    if (!ctx) return

    const cw = canvas.clientWidth
    const ch = canvas.clientHeight
    if (canvas.width !== cw || canvas.height !== ch) {
      canvas.width = cw
      canvas.height = ch
    }

    const now = performance.now()
    const dt = Math.min(100, now - lastFrameTimeRef.current) / 1000
    lastFrameTimeRef.current = now

    // Smooth factor (lerp speed)
    const t = 1 - Math.exp(-15 * dt)

    const v = viewRef.current
    const fitScale = Math.min(cw / minimap.width, ch / minimap.height)
    const scale = fitScale * v.zoom
    const dw = minimap.width * scale
    const dh = minimap.height * scale
    const dx = (cw - dw) / 2 + v.panX
    const dy = (ch - dh) / 2 + v.panY
    layoutRef.current = { dx, dy, scale, w: minimap.width, h: minimap.height }

    ctx.imageSmoothingEnabled = false
    ctx.fillStyle = "#0a0f18"
    ctx.fillRect(0, 0, cw, ch)
    if (layer) {
      ctx.drawImage(layer, dx, dy, dw, dh)
    }

    const atlas = atlasImgRef.current
    const meta = atlasMetaRef.current
    if (atlas && meta && scale >= 3) {
      const cell = meta.cell
      const fg = minimap.foreground_tiles
      const bg = minimap.background_tiles
      const xMin = Math.max(0, Math.floor(-dx / scale))
      const xMax = Math.min(minimap.width, Math.ceil((cw - dx) / scale))
      const yScreenMin = Math.max(0, Math.floor(-dy / scale))
      const yScreenMax = Math.min(minimap.height, Math.ceil((ch - dy) / scale))
      const yMin = minimap.height - yScreenMax
      const yMax = minimap.height - yScreenMin

      ctx.globalAlpha = 0.5
      for (let y = yMin; y < yMax; y += 1) {
        for (let x = xMin; x < xMax; x += 1) {
          const bgId = bg[y * minimap.width + x]
          if (!bgId) continue
          const pos = meta.tiles[String(bgId)]
          if (!pos) continue
          const screenY = minimap.height - 1 - y
          ctx.drawImage(
            atlas,
            pos[0],
            pos[1],
            cell,
            cell,
            dx + x * scale,
            dy + screenY * scale,
            scale,
            scale,
          )
        }
      }
      ctx.globalAlpha = 1.0

      for (let y = yMin; y < yMax; y += 1) {
        for (let x = xMin; x < xMax; x += 1) {
          const fgId = fg[y * minimap.width + x]
          if (!fgId) continue
          const pos = meta.tiles[String(fgId)]
          if (!pos) continue
          const screenY = minimap.height - 1 - y
          ctx.drawImage(
            atlas,
            pos[0],
            pos[1],
            cell,
            cell,
            dx + x * scale,
            dy + screenY * scale,
            scale,
            scale,
          )
        }
      }
    }

    if (hover) {
      const hx = dx + hover.mapX * scale
      const hy = dy + (minimap.height - hover.mapY - 1) * scale
      ctx.strokeStyle = "#ffffff"
      ctx.lineWidth = 1.5
      ctx.strokeRect(hx, hy, scale, scale)
    }

    // Draw Collectables (Dropped Items)
    for (const c of collectables) {
        // pos_x and pos_y are world coords; convert to map coords for rendering
        // TILE_WIDTH = 0.32, TILE_HEIGHT = 0.32. 
        // Map X = world_x / 0.32
        // Map Y = (world_y + 0.16) / 0.32
        const mapX = c.pos_x / 0.32;
        const mapY = (c.pos_y + 0.16) / 0.32;
        
        // Use floor to match backend pathfinding tile
        const cx = Math.floor(mapX);
        const cy = Math.floor(mapY);

        const sx = dx + cx * scale + scale / 2;
        const sy = dy + (minimap.height - cy - 1) * scale + scale / 2;

        const isNugget = [4154, 4155, 4156, 4157, 4162].includes(c.block_type || 0);
        ctx.fillStyle = isNugget ? "#facc15" : "#a855f7"; // yellow-400 for nuggets, purple-500 for gems
        ctx.beginPath();
        // Draw them slightly smaller than players so they look like dropped items
        ctx.arc(sx, sy, Math.max(1.5, scale * 0.3), 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = isNugget ? "#ca8a04" : "#7e22ce";
        ctx.lineWidth = 1;
        ctx.stroke();
    }

    // Draw Other Players with Lerp
    for (const op of otherPlayers) {
      if (op.position.map_x == null || op.position.map_y == null) continue
      let visual = visualOthersRef.current.get(op.user_id)
      if (!visual) {
        visual = { x: op.position.map_x, y: op.position.map_y }
        visualOthersRef.current.set(op.user_id, visual)
      }
      visual.x += (op.position.map_x - visual.x) * t
      visual.y += (op.position.map_y - visual.y) * t

      const sx = dx + visual.x * scale + scale / 2
      const sy = dy + (minimap.height - visual.y - 1) * scale + scale / 2
      ctx.fillStyle = "#a855f7"
      ctx.beginPath()
      ctx.arc(sx, sy, Math.max(2, scale * 0.45), 0, Math.PI * 2)
      ctx.fill()
    }

    // Draw AI Enemies with Lerp
    const currentAiIds = new Set(aiEnemies.map(e => e.ai_id))
    for (const [id] of visualEnemiesRef.current) {
      if (!currentAiIds.has(id)) visualEnemiesRef.current.delete(id)
    }

    for (const enemy of aiEnemies) {
      if (!enemy.alive) continue
      let visual = visualEnemiesRef.current.get(enemy.ai_id)
      if (!visual) {
        visual = { x: enemy.map_x, y: enemy.map_y }
        visualEnemiesRef.current.set(enemy.ai_id, visual)
      }
      visual.x += (enemy.map_x - visual.x) * t
      visual.y += (enemy.map_y - visual.y) * t

      const sx = dx + visual.x * scale + scale / 2
      const sy = dy + (minimap.height - visual.y - 1) * scale + scale / 2
      ctx.fillStyle = "#fb923c"
      ctx.beginPath()
      ctx.arc(sx, sy, Math.max(2, scale * 0.4), 0, Math.PI * 2)
      ctx.fill()
      ctx.strokeStyle = "#ea580c"
      ctx.lineWidth = 1
      ctx.stroke()
    }

    // Draw Local Player with Lerp
    const px = playerPosition.map_x
    const py = playerPosition.map_y
    if (px != null && py != null) {
      if (!visualPlayerRef.current) {
        visualPlayerRef.current = { x: px, y: py }
      }
      visualPlayerRef.current.x += (px - visualPlayerRef.current.x) * t
      visualPlayerRef.current.y += (py - visualPlayerRef.current.y) * t

      const sx = dx + visualPlayerRef.current.x * scale + scale / 2
      const sy = dy + (minimap.height - visualPlayerRef.current.y - 1) * scale + scale / 2
      ctx.fillStyle = "#ff3b30"
      ctx.beginPath()
      ctx.arc(sx, sy, Math.max(3, scale * 0.6), 0, Math.PI * 2)
      ctx.fill()
      ctx.strokeStyle = "#ffffff"
      ctx.lineWidth = 1
      ctx.stroke()
    }

    // Draw Current Target
    if (currentTarget && minimap) {
      const tx = currentTarget.x
      const ty = currentTarget.y
      const sx = dx + tx * scale + scale / 2
      const sy = dy + (minimap.height - ty - 1) * scale + scale / 2

      const pulse = 1 + Math.sin(now / 200) * 0.2
      ctx.strokeStyle = "#10b981" // emerald-500
      ctx.lineWidth = 2
      ctx.beginPath()
      ctx.arc(sx, sy, scale * 0.8 * pulse, 0, Math.PI * 2)
      ctx.stroke()

      // Crosshair lines
      const len = scale * 0.5
      ctx.beginPath()
      ctx.moveTo(sx - len, sy)
      ctx.lineTo(sx + len, sy)
      ctx.moveTo(sx, sy - len)
      ctx.lineTo(sx, sy + len)
      ctx.stroke()
    }
  }, [minimap, hover, playerPosition.map_x, playerPosition.map_y, aiEnemies, otherPlayers, currentTarget, collectables])

  const drawSceneRef = useRef(drawScene)
  drawSceneRef.current = drawScene

  // Animation Loop for smoothness
  useEffect(() => {
    let frameId: number
    const loop = () => {
      drawSceneRef.current()
      frameId = requestAnimationFrame(loop)
    }
    frameId = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(frameId)
  }, [])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const observer = new ResizeObserver(() => drawSceneRef.current())
    observer.observe(canvas)
    return () => observer.disconnect()
  }, [])

  const screenToTile = useCallback(
    (clientX: number, clientY: number): TileInfo | null => {
      if (!minimap) return null
      const canvas = canvasRef.current
      if (!canvas) return null
      const rect = canvas.getBoundingClientRect()
      const cx = clientX - rect.left
      const cy = clientY - rect.top
      const { dx, dy, scale, w, h } = layoutRef.current
      const mapX = Math.floor((cx - dx) / scale)
      const screenY = Math.floor((cy - dy) / scale)
      const mapY = h - 1 - screenY
      if (mapX < 0 || mapX >= w || mapY < 0 || mapY >= h) return null
      const i = mapY * w + mapX
      return {
        mapX,
        mapY,
        fg: minimap.foreground_tiles[i] ?? 0,
        bg: minimap.background_tiles[i] ?? 0,
        water: minimap.water_tiles[i] ?? 0,
        wiring: minimap.wiring_tiles[i] ?? 0,
      }
    },
    [minimap],
  )

  const dragRef = useRef<{
    startX: number
    startY: number
    panX0: number
    panY0: number
    moved: boolean
  } | null>(null)

  const lastHoverKeyRef = useRef<string>("")
  const updateHover = useCallback((tile: TileInfo | null) => {
    const key = tile ? `${tile.mapX},${tile.mapY}` : ""
    if (key === lastHoverKeyRef.current) return
    lastHoverKeyRef.current = key
    setHover(tile)
    if (!tile) {
      onHoverChangeRef.current("Hover a tile to inspect it.")
      return
    }
    onHoverChangeRef.current(
      `hover tile=(${tile.mapX}, ${tile.mapY}) fg=${tile.fg} bg=${tile.bg} water=${tile.water} wiring=${tile.wiring}`,
    )
  }, [])

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return
    const v = viewRef.current
    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      panX0: v.panX,
      panY0: v.panY,
      moved: false,
    }
      ; (e.currentTarget as HTMLCanvasElement).setPointerCapture(e.pointerId)
  }, [])

  const onPointerMove = useCallback(
    (e: React.PointerEvent) => {
      const tile = screenToTile(e.clientX, e.clientY)
      updateHover(tile)
      const drag = dragRef.current
      if (!drag) return
      const dx = e.clientX - drag.startX
      const dy = e.clientY - drag.startY
      if (!drag.moved && Math.abs(dx) + Math.abs(dy) > 5) {
        drag.moved = true
      }
      if (drag.moved) {
        e.preventDefault()
        setView((prev) => ({ ...prev, panX: drag.panX0 + dx, panY: drag.panY0 + dy }))
      }
    },
    [screenToTile, updateHover],
  )

  const onPointerUp = useCallback((e: React.PointerEvent) => {
    if (dragRef.current) {
      try {
        ; (e.currentTarget as HTMLCanvasElement).releasePointerCapture(e.pointerId)
      } catch {
        /* noop */
      }
    }
    dragRef.current = null
  }, [])

  const onPointerLeave = useCallback(() => {
    updateHover(null)
  }, [updateHover])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const handler = (e: WheelEvent) => {
      if (!minimapRef.current) return
      e.preventDefault()
      const rect = canvas.getBoundingClientRect()
      const cx = e.clientX - rect.left
      const cy = e.clientY - rect.top
      const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15
      const halfW = canvas.clientWidth / 2
      const halfH = canvas.clientHeight / 2
      setView((v) => {
        const nextZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, v.zoom * factor))
        const ratio = v.zoom > 0 ? nextZoom / v.zoom : 1
        return {
          zoom: nextZoom,
          panX: cx - halfW - (cx - halfW - v.panX) * ratio,
          panY: cy - halfH - (cy - halfH - v.panY) * ratio,
        }
      })
    }
    canvas.addEventListener("wheel", handler, { passive: false })
    return () => canvas.removeEventListener("wheel", handler)
  }, [])

  return (
    <div className="relative flex h-80 overflow-hidden rounded-2xl border border-white/10 bg-[#081018]">
      <div className="relative flex-1 overflow-hidden">
        <canvas
          ref={canvasRef}
          className="absolute inset-0 h-full w-full cursor-crosshair"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onPointerLeave={onPointerLeave}
          style={{ touchAction: "none" }}
        />
        {!minimap && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
            No minimap yet.
          </div>
        )}
      </div>

      <div className="w-64 border-l border-white/10 bg-black/40 p-4 backdrop-blur-md flex flex-col gap-4">
        <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
          <Crosshair className="size-3.5" />
          Target Intelligence
        </div>

        {currentTarget ? (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-3 rounded-xl bg-emerald-500/10 border border-emerald-500/20 p-3">
              <Target className="size-5 text-emerald-400 animate-pulse" />
              <div className="flex flex-col">
                <span className="text-[10px] uppercase font-bold text-emerald-400/70">Current Action</span>
                <span className="text-xs font-bold text-emerald-100 capitalize">
                  {currentTarget.type === "collecting"
                    ? ([4154, 4155, 4156, 4157, 4162].includes(currentTarget.block_id || 0) ? "Nugget" : "Gem/Item")
                    : currentTarget.type}
                </span>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="flex flex-col rounded-xl bg-white/5 border border-white/10 p-2">
                <span className="text-[9px] uppercase font-bold text-muted-foreground">Map X</span>
                <span className="text-xs font-mono font-bold text-white">{currentTarget.x}</span>
              </div>
              <div className="flex flex-col rounded-xl bg-white/5 border border-white/10 p-2">
                <span className="text-[9px] uppercase font-bold text-muted-foreground">Map Y</span>
                <span className="text-xs font-mono font-bold text-white">{currentTarget.y}</span>
              </div>
            </div>

            {currentTarget.type === "collecting" && (
              <div className="flex flex-col rounded-xl bg-white/5 border border-white/10 p-2">
                <span className="text-[9px] uppercase font-bold text-muted-foreground">Entity ID</span>
                <span className="text-xs font-mono font-bold text-white">#{currentTarget.id}</span>
              </div>
            )}
            {currentTarget.type === "fighting" && (
              <div className="flex flex-col rounded-xl bg-white/5 border border-white/10 p-2">
                <span className="text-[9px] uppercase font-bold text-muted-foreground">AI Enemy ID</span>
                <span className="text-xs font-mono font-bold text-white">#{currentTarget.ai_id}</span>
              </div>
            )}
          </div>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 opacity-50">
            <div className="size-8 rounded-full border-2 border-dashed border-white/20 animate-spin-slow" />
            <span className="text-[10px] uppercase font-bold text-muted-foreground">Waiting for target...</span>
          </div>
        )}
      </div>
    </div>
  )
}

export const MinimapPanel = memo(MinimapPanelImpl, (prev, next) => {
  return (
    prev.minimap === next.minimap &&
    prev.onHoverChange === next.onHoverChange &&
    prev.currentWorld === next.currentWorld &&
    prev.playerPosition.map_x === next.playerPosition.map_x &&
    prev.playerPosition.map_y === next.playerPosition.map_y &&
    prev.playerPosition.world_x === next.playerPosition.world_x &&
    prev.playerPosition.world_y === next.playerPosition.world_y &&
    prev.aiEnemies === next.aiEnemies &&
    prev.otherPlayers === next.otherPlayers &&
    prev.currentTarget === next.currentTarget
  )
})
