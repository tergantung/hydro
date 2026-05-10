import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  Bug,
  ChatCenteredDots,
  Fish,
  GameController,
  Gear,
  Moon,
  NavigationArrow,
  Plug,
  SpinnerGap,
  List,
  CaretRight,
  Waves,
  Code,
  LockKey,
  Robot,
  Globe,
  ShieldCheck,
} from "@phosphor-icons/react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { MinimapPanel } from "@/components/minimap"
import { TileSprite } from "@/components/TileSprite"
import {
  automateTutorial,
  connectWithAuth,
  disconnectSession,
  getDashboardAuthStatus,
  getMinimap,
  getAuthToken,
  joinWorld,
  leaveWorld,
  loginDashboard,
  listSessions,
  loadBlockTypes,
  logoutDashboard,
  moveSession,
  registerDashboardPassword,
  setAuthToken,
  getLuaScriptStatus,
  reconnectSession,
  startAutomine,
  stopAutomine,
  startAutoclear,
  stopAutoclear,
  startFishing,
  startLuaScript,
  startSpam,
  stopLuaScript,
  stopFishing,
  stopSpam,
  talk,
  wearItem,
  dropItem,
  deleteSession,
  startAutonether,
  stopAutonether,
  getAutonetherStatus,
  setAutomineSpeed,
  type ActionResponse,
} from "@/lib/api"
import {
  buildWebSocketUrl,
  camelToWords,
  generateDeviceId,
  getItemCategory,
  inventoryTypeLabel,
  sortSessions,
  statusVariant,
} from "@/lib/dashboard"
import type {
  AuthInput,
  AuthKind,
  BlockNameMap,
  DashboardAuthStatus,
  LuaScriptStatusSnapshot,
  MinimapSnapshot,
  ServerEvent,
  SessionSnapshot,
  TutorialCompletedEvent,
} from "@/lib/types"

type Feedback = {
  kind: "success" | "error"
  message: string
}

type SessionInputs = {
  world: string
  autoclearWorld: string
  bait: string
  chat: string
  luaSource: string
  spam: string
  spamDelay: string
}

const EMPTY_INPUTS: SessionInputs = {
  world: "",
  autoclearWorld: "",
  bait: "",
  chat: "",
  luaSource: "",
  spam: "",
  spamDelay: "5",
}

const CLOTHING_TYPES = new Set([768, 1024])




function App() {
  const [authKind, setAuthKind] = useState<AuthKind>("none")
  const [deviceId, setDeviceId] = useState("")
  const [jwt, setJwt] = useState("")
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [proxy, setProxy] = useState("")
  const [dashboardPassword, setDashboardPassword] = useState("")
  const [dashboardCode, setDashboardCode] = useState<string | null>(null)
  const [countdown, setCountdown] = useState<number | null>(null)
  const [dashboardStatus, setDashboardStatus] = useState<DashboardAuthStatus | null>(null)
  const [dashboardToken, setDashboardToken] = useState<string | null>(getAuthToken())
  const [dashboardBusy, setDashboardBusy] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [mainView, setMainView] = useState<"sessions" | "scripting">("sessions")
  const [createSessionExpanded, setCreateSessionExpanded] = useState(true)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [sessions, setSessions] = useState<SessionSnapshot[]>([])
  const [sessionInputs, setSessionInputs] = useState<Record<string, SessionInputs>>({})
  const [minimaps, setMinimaps] = useState<Record<string, MinimapSnapshot | null>>({})
  const [luaStatuses, setLuaStatuses] = useState<Record<string, LuaScriptStatusSnapshot | null>>(
    {},
  )
  const [autonetherStatuses, setAutonetherStatuses] = useState<Record<string, { active: boolean; phase: string } | null>>({})
  const [hoverTiles, setHoverTiles] = useState<Record<string, string>>({})
  const [dropAmounts, setDropAmounts] = useState<Record<string, string>>({})
  const [automineSpeed, setAutomineSpeedState] = useState<Record<string, number>>({})

  const [blockNames, setBlockNames] = useState<BlockNameMap>({})
  const [feedback, setFeedback] = useState<Feedback | null>(null)
  const [tutorialCompleted, setTutorialCompleted] =
    useState<TutorialCompletedEvent | null>(null)
  const [connecting, setConnecting] = useState(false)
  const [activeSessionId, setActiveSessionId] = useState<string>("")
  const [maintenance, setMaintenance] = useState<{ active: boolean; message: string } | null>(null)
  const reconnectTimerRef = useRef<number | null>(null)


  const dashboardAuthenticated = dashboardStatus?.authenticated ?? false

  const refreshDashboardStatus = useCallback(async () => {
    try {
      // Check app status (maintenance/version)
      const appStatus = await fetch('/api/status').then(r => r.json())
      if (appStatus.config?.maintenance) {
        setMaintenance({ active: true, message: appStatus.config.maintenance_message })
      }

      const status = await getDashboardAuthStatus()
      setDashboardStatus(status)
      if (!status.authenticated) {
        setDashboardToken(null)
        setAuthToken(null)
      }
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : "dashboard auth failed",
      })
    }
  }, [])

  const upsertSession = useCallback((snapshot: SessionSnapshot) => {
    setSessions((current) => {
      const next = [...current]
      const index = next.findIndex((session) => session.id === snapshot.id)
      if (index === -1) {
        next.push(snapshot)
      } else {
        next[index] = snapshot
      }
      return sortSessions(next)
    })
  }, [])

  const updateFromAction = useCallback((response: ActionResponse) => {
    if (response.session) {
      upsertSession(response.session)
    }
    if (response.result?.message) {
      setFeedback({ kind: "success", message: response.result.message })
    }
  }, [upsertSession])

  useEffect(() => {
    void refreshDashboardStatus()
  }, [refreshDashboardStatus])

  useEffect(() => {
    if (!dashboardAuthenticated) {
      return
    }
    void Promise.all([listSessions(), loadBlockTypes()])
      .then(([sessionPayload, blockPayload]) => {
        setSessions(sortSessions(sessionPayload.sessions ?? []))
        setBlockNames(blockPayload)
      })
      .catch((error: Error) => {
        setFeedback({ kind: "error", message: error.message })
      })
  }, [dashboardAuthenticated])

  useEffect(() => {
    setSessionInputs((current) => {
      const next = { ...current }
      for (const session of sessions) {
        if (!next[session.id]) {
          next[session.id] = {
            ...EMPTY_INPUTS,
            world: session.current_world ?? "",
          }
          continue
        }
        if (!next[session.id].world && session.current_world) {
          next[session.id] = {
            ...next[session.id],
            world: session.current_world,
          }
        }
      }
      return next
    })
  }, [sessions])

  useEffect(() => {
    if (!sessions.length) {
      setActiveSessionId("")
      return
    }

    setActiveSessionId((current) =>
      current && sessions.some((session) => session.id === current)
        ? current
        : sessions[0].id,
    )
  }, [sessions])

  useEffect(() => {
    if (!dashboardAuthenticated) {
      return
    }
    let cancelled = false
    let socket: WebSocket | null = null
    let reconnectDelay = 1000

    const connect = () => {
      if (cancelled) {
        return
      }

      socket = new WebSocket(buildWebSocketUrl(dashboardToken))

      socket.onopen = () => {
        reconnectDelay = 1000
        setFeedback((current) =>
          current?.message === "dashboard websocket disconnected" ? null : current,
        )
      }

      socket.onmessage = (event) => {
        const payload = JSON.parse(event.data) as ServerEvent
        if (payload.type === "log") {
          // Log state updates disabled to prevent UI lag as per user request
          // setLogs((current) => [...current.slice(-499), payload.event])
          return
        }
        if (payload.type === "session") {
          upsertSession(payload.snapshot)
          return
        }
        setTutorialCompleted(payload.event)
      }

      socket.onclose = () => {
        if (cancelled) {
          return
        }
        setFeedback({ kind: "error", message: "dashboard websocket disconnected" })
        reconnectTimerRef.current = window.setTimeout(() => {
          reconnectDelay = Math.min(reconnectDelay * 2, 5000)
          connect()
        }, reconnectDelay)
      }
    }

    connect()

    return () => {
      cancelled = true
      if (reconnectTimerRef.current !== null) {
        window.clearTimeout(reconnectTimerRef.current)
        reconnectTimerRef.current = null
      }
      socket?.close()
    }
  }, [dashboardAuthenticated, dashboardToken, upsertSession])

  const minimapFetchAtRef = useRef<Record<string, number>>({})
  const minimapFetchedWorldRef = useRef<Record<string, string | null>>({})
  const minimapWorldChangedAtRef = useRef<Record<string, number>>({})

  const refreshMinimap = useCallback(async (sessionId: string, currentWorld: string | null) => {
    const lastWorld = minimapFetchedWorldRef.current[sessionId]
    const worldChanged = lastWorld !== currentWorld
    if (worldChanged) {
      minimapFetchAtRef.current[sessionId] = 0
      minimapFetchedWorldRef.current[sessionId] = currentWorld
      minimapWorldChangedAtRef.current[sessionId] = Date.now()
      setMinimaps((current) => {
        if (current[sessionId] == null) return current
        return { ...current, [sessionId]: null }
      })
    }
    const now = Date.now()
    const last = minimapFetchAtRef.current[sessionId] ?? 0
    const recentChange = minimapWorldChangedAtRef.current[sessionId] ?? 0
    const inWarmup = now - recentChange < 10_000
    const minGap = inWarmup ? 500 : 4000
    if (now - last < minGap) {
      return
    }
    minimapFetchAtRef.current[sessionId] = now
    try {
      const payload = await getMinimap(sessionId)
      const snap = payload.minimap ?? null
      setMinimaps((current) => ({
        ...current,
        [sessionId]: snap,
      }))
      if (!snap) {
        minimapFetchAtRef.current[sessionId] = now - (minGap - 500)
      }
    } catch {
      setMinimaps((current) => ({
        ...current,
        [sessionId]: null,
      }))
      minimapFetchAtRef.current[sessionId] = now - (minGap - 500)
    }
  }, [])

  const handleHoverChange = useCallback((sessionId: string, value: string) => {
    setHoverTiles((current) => {
      if (current[sessionId] === value) return current
      return { ...current, [sessionId]: value }
    })
  }, [])

  const hoverChangeBySessionRef = useRef<Record<string, (value: string) => void>>({})

  const getHoverChange = useCallback((sessionId: string) => {
    let bound = hoverChangeBySessionRef.current[sessionId]
    if (!bound) {
      bound = (value: string) => handleHoverChange(sessionId, value)
      hoverChangeBySessionRef.current[sessionId] = bound
    }
    return bound
  }, [handleHoverChange])

  const refreshLuaStatus = useCallback(async (sessionId: string) => {
    try {
      const payload = await getLuaScriptStatus(sessionId)
      setLuaStatuses((current) => ({
        ...current,
        [sessionId]: payload.status ?? null,
      }))
    } catch {
      setLuaStatuses((current) => ({
        ...current,
        [sessionId]: null,
      }))
    }
  }, [])

  const refreshAutonetherStatus = useCallback(async (sessionId: string) => {
    try {
      const payload = await getAutonetherStatus(sessionId)
      setAutonetherStatuses((current) => ({
        ...current,
        [sessionId]: payload.status ?? null,
      }))
    } catch {
      setAutonetherStatuses((current) => ({
        ...current,
        [sessionId]: null,
      }))
    }
  }, [])

  useEffect(() => {
    sessions.forEach((session) => {
      if (
        session.status === "in_world" ||
        session.status === "awaiting_ready" ||
        session.status === "loading_world"
      ) {
        void refreshMinimap(session.id, session.current_world)
      } else if (!session.current_world) {
        setMinimaps((current) => ({ ...current, [session.id]: null }))
      }
    })
  }, [refreshMinimap, sessions])

  useEffect(() => {
    sessions.forEach((session) => {
      void refreshLuaStatus(session.id)
    })
  }, [refreshLuaStatus, sessions])

  useEffect(() => {
    sessions.forEach((session) => {
      void refreshAutonetherStatus(session.id)
    })
  }, [refreshAutonetherStatus, sessions])

  const runAction = useCallback(
    async (action: () => Promise<ActionResponse>) => {
      try {
        const response = await action()
        updateFromAction(response)
      } catch (error) {
        if (error instanceof Error && error.message.toLowerCase().includes("unauthorized")) {
          await refreshDashboardStatus()
        }
        setFeedback({
          kind: "error",
          message: error instanceof Error ? error.message : "request failed",
        })
      }
    },
    [refreshDashboardStatus, updateFromAction],
  )

  const runLuaAction = useCallback(
    async (
      sessionId: string,
      action: () => Promise<{ result?: { message?: string }; status?: LuaScriptStatusSnapshot | null }>,
    ) => {
      try {
        const response = await action()
        if (response.result?.message) {
          setFeedback({ kind: "success", message: response.result.message })
        }
        setLuaStatuses((current) => ({
          ...current,
          [sessionId]: response.status ?? current[sessionId] ?? null,
        }))
        await refreshLuaStatus(sessionId)
      } catch (error) {
        if (error instanceof Error && error.message.toLowerCase().includes("unauthorized")) {
          await refreshDashboardStatus()
        }
        setFeedback({
          kind: "error",
          message: error instanceof Error ? error.message : "request failed",
        })
      }
    },
    [refreshDashboardStatus, refreshLuaStatus],
  )



  const createAuthInput = useMemo<AuthInput>(() => {
    if (authKind === "jwt") {
      return {
        kind: "jwt",
        jwt,
        device_id: deviceId || null,
      }
    }
    if (authKind === "email_password") {
      return {
        kind: "email_password",
        email,
        password,
        device_id: deviceId || null,
      }
    }
    return {
      kind: "android_device",
      device_id: deviceId || null,
    }
  }, [authKind, deviceId, email, jwt, password])

  const handleConnect = async () => {
    setConnecting(true)
    try {
      const response = await connectWithAuth(createAuthInput, proxy || undefined)
      updateFromAction(response)
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : "connect failed",
      })
    } finally {
      setConnecting(false)
    }
  }



  const handleDashboardLogin = async () => {
    setDashboardBusy(true)
    try {
      const response = await loginDashboard(dashboardPassword)
      if (response.token) {
        setAuthToken(response.token)
        setDashboardToken(response.token)
      }
      await refreshDashboardStatus()
      setDashboardPassword("")
      if (response.message) {
        setFeedback({ kind: "success", message: response.message })
      }
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : "login failed",
      })
    } finally {
      setDashboardBusy(false)
    }
  }

  const handleCreateCode = async () => {
    setDashboardBusy(true)
    const code = Math.floor(100000 + Math.random() * 900000).toString()
    setDashboardCode(code)
    
    // Save code to code.txt file
    try {
      await fetch('/api/save-code', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code })
      }).catch(() => {
        // Fallback: save to localStorage if API fails
        localStorage.setItem('hydro_access_code', code)
      })
    } catch {
      localStorage.setItem('hydro_access_code', code)
    }
    
    try {
      const response = await registerDashboardPassword(code)
      if (response.token) {
        // We will login after the countdown
      }
      setFeedback({ kind: "success", message: "Code created! Redirecting soon..." })
      
      let timeLeft = 10
      setCountdown(timeLeft)
      const timer = setInterval(() => {
        setCountdown((prev) => {
          if (prev === null || prev <= 1) {
            clearInterval(timer)
            void (async () => {
              try {
                const loginResponse = await loginDashboard(code)
                if (loginResponse.token) {
                  setAuthToken(loginResponse.token)
                  setDashboardToken(loginResponse.token)
                }
                await refreshDashboardStatus()
              } catch (error) {
                setFeedback({ kind: "error", message: "Auto-login failed" })
              } finally {
                setDashboardBusy(false)
                setCountdown(null)
              }
            })()
            return 0
          }
          return prev - 1
        })
      }, 1000)

    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : "register failed",
      })
      setDashboardBusy(false)
    }
  }

  const handleDashboardLogout = async () => {
    setDashboardBusy(true)
    try {
      await logoutDashboard()
    } catch {
      // ignore
    } finally {
      setAuthToken(null)
      setDashboardToken(null)
      await refreshDashboardStatus()
      setDashboardBusy(false)
    }
  }

  if (!dashboardStatus || !dashboardAuthenticated) {
    const registered = dashboardStatus?.registered ?? false
    return (
      <div className="min-h-screen bg-[#0B0D0C] text-foreground selection:bg-primary/30 relative overflow-hidden">
        {/* Animated Background */}
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top_right,rgba(138,132,86,0.08),transparent_50%),radial-gradient(ellipse_at_bottom_left,rgba(194,182,106,0.05),transparent_50%)]" />
        <div className="absolute inset-0 bg-[url('data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAwIiBoZWlnaHQ9IjIwMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48ZGVmcz48cGF0dGVybiBpZD0iZ3JpZCIgd2lkdGg9IjQwIiBoZWlnaHQ9IjQwIiBwYXR0ZXJuVW5pdHM9InVzZXJTcGFjZU9uVXNlIj48cGF0aCBkPSJNIDQwIDAgTCAwIDAgMCA0MCIgZmlsbD0ibm9uZSIgc3Ryb2tlPSJyZ2JhKDE5NCwxODIsMTA2LDAuMDMpIiBzdHJva2Utd2lkdGg9IjEiLz48L3BhdHRlcm4+PC9kZWZzPjxyZWN0IHdpZHRoPSIxMDAlIiBoZWlnaHQ9IjEwMCUiIGZpbGw9InVybCgjZ3JpZCkiLz48L3N2Zz4=')] opacity-20" />
        
        <div className="mx-auto flex min-h-screen max-w-[640px] items-center px-4 py-10 relative z-10">
          <Card className="w-full glass-dark ring-1 ring-border hydro-border-glow">
            <CardHeader className="space-y-4">
              <CardTitle className="flex items-center gap-3 text-3xl font-bold tracking-tight">
                <div className="relative">
                  <Waves className="size-8 text-primary animate-pulse hydro-glow-sm" />
                  <div className="absolute inset-0 bg-primary/20 blur-xl rounded-full" />
                </div>
                <span className="text-gradient">Hydro</span>
              </CardTitle>
              <CardDescription className="text-base">
                {registered
                  ? "Enter your 6-digit code to unlock the dashboard."
                  : "Create your own code to protect the dashboard."}
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-6">
              {dashboardCode ? (
                <div className="flex flex-col items-center gap-4 py-8">
                  <div className="text-xs text-muted-foreground uppercase tracking-[0.3em] font-bold">Your 6-Digit Code</div>
                  <div className="relative">
                    <div className="text-8xl font-black tracking-[0.25em] text-primary font-mono hydro-glow">
                      {dashboardCode}
                    </div>
                    <div className="absolute inset-0 bg-primary/10 blur-3xl -z-10" />
                  </div>
                  <div className="text-xs text-muted-foreground mt-10 flex items-center gap-3 px-4 py-2 rounded-lg bg-black/40 border border-primary/20">
                    <SpinnerGap className="size-4 animate-spin text-primary" />
                    <span>Entering dashboard in <span className="text-primary font-bold text-base mx-1">{countdown}</span> seconds...</span>
                  </div>
                  <div className="text-[10px] text-muted-foreground/60 mt-4 text-center max-w-sm">
                    Code will be saved to <span className="text-primary font-mono">code.txt</span>
                  </div>
                </div>
              ) : (
                <>
                  <div className="grid gap-3">
                    <label className="text-xs text-muted-foreground uppercase tracking-wider">Access Code</label>
                    <Input
                      type="text"
                      maxLength={6}
                      value={dashboardPassword}
                      onChange={(event) => setDashboardPassword(event.target.value.replace(/\D/g, "").slice(0, 6))}
                      placeholder="000000"
                      className="rounded-xl border-border bg-black/40 text-center text-4xl tracking-[0.6em] font-mono h-20 hydro-border-glow focus:ring-primary/50"
                    />
                  </div>
                  <div className="flex flex-col gap-3">
                    <Button
                      onClick={() => void handleDashboardLogin()}
                      disabled={dashboardBusy || dashboardPassword.length < 6}
                      className="rounded-xl h-14 text-lg font-bold bg-primary/20 hover:bg-primary/30 border border-primary/50 text-primary hydro-glow-sm transition-all"
                    >
                      {dashboardBusy ? (
                        <SpinnerGap className="size-6 animate-spin" />
                      ) : (
                        <LockKey className="size-6" />
                      )}
                      <span className="ml-2">Unlock Dashboard</span>
                    </Button>
                    {!registered && (
                      <Button
                        variant="outline"
                        onClick={() => void handleCreateCode()}
                        disabled={dashboardBusy}
                        className="rounded-xl border-primary/40 bg-black/40 text-primary hover:bg-primary/10 h-12 font-semibold"
                      >
                        Create your own code
                      </Button>
                    )}
                  </div>
                </>
              )}
              {feedback && !dashboardCode ? (
                <div
                  className={`rounded-xl border px-4 py-3 text-sm font-medium ${
                    feedback.kind === "error"
                      ? "hydro-error-banner text-[#E7E4D8]"
                      : "border-[#6F8B57]/40 bg-[#6F8B57]/10 text-[#E7E4D8] hydro-success-glow"
                  }`}
                >
                  {feedback.message}
                </div>
              ) : null}
            </CardContent>
          </Card>
        </div>
      </div>
    )
  }

  const mutateSessionInput = (
    sessionId: string,
    field: keyof SessionInputs,
    value: string,
  ) => {
    setSessionInputs((current) => ({
      ...current,
      [sessionId]: {
        ...(current[sessionId] ?? EMPTY_INPUTS),
        [field]: value,
      },
    }))
  }

  const parseSpamDelayMs = (rawValue: string) => {
    const seconds = Number(rawValue)
    if (!Number.isFinite(seconds) || seconds <= 0) {
      throw new Error("spam delay must be a number greater than 0 seconds")
    }
    const delayMs = Math.round(seconds * 1000)
    if (delayMs < 250) {
      throw new Error("spam delay must be at least 0.25 seconds")
    }
    return delayMs
  }

  if (maintenance?.active) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-[#0a0a0c] p-6 text-foreground selection:bg-primary/30">
        <div className="fixed inset-0 -z-10 bg-[radial-gradient(circle_at_50%_50%,rgba(50,50,80,0.15),transparent)]" />
        <Card className="max-w-md w-full border-white/10 bg-card/50 backdrop-blur-3xl ring-1 ring-white/10 hydro-glow-lg">
          <CardHeader className="text-center pb-2">
            <div className="mx-auto mb-4 flex size-16 items-center justify-center rounded-2xl bg-primary/10 ring-1 ring-primary/20">
              <Gear className="size-8 text-primary animate-spin-slow" />
            </div>
            <CardTitle className="text-2xl font-bold tracking-tight text-gradient">System Maintenance</CardTitle>
            <CardDescription className="text-muted-foreground mt-2">
              Hydro is currently undergoing scheduled maintenance or updates.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-6 pt-4">
            <div className="rounded-xl border border-primary/20 bg-primary/5 p-4 text-center">
              <p className="text-sm text-primary/90 font-medium">
                "{maintenance.message}"
              </p>
            </div>
            <div className="space-y-2 text-center text-xs text-muted-foreground">
              <p>Please check back later. We are working hard to bring you the best experience.</p>
              <div className="flex justify-center gap-4 mt-4 text-primary">
                <a href="#" className="hover:underline transition-all">Discord</a>
                <a href="#" className="hover:underline transition-all">Updates</a>
                <a href="#" className="hover:underline transition-all">Support</a>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-[#0B0D0C] text-foreground selection:bg-primary/30 relative overflow-hidden">
      {/* Animated Background */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top_right,rgba(138,132,86,0.08),transparent_50%),radial-gradient(ellipse_at_bottom_left,rgba(194,182,106,0.05),transparent_50%)]" />
      <div className="absolute inset-0 bg-[url('data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAwIiBoZWlnaHQ9IjIwMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48ZGVmcz48cGF0dGVybiBpZD0iZ3JpZCIgd2lkdGg9IjQwIiBoZWlnaHQ9IjQwIiBwYXR0ZXJuVW5pdHM9InVzZXJTcGFjZU9uVXNlIj48cGF0aCBkPSJNIDQwIDAgTCAwIDAgMCA0MCIgZmlsbD0ibm9uZSIgc3Ryb2tlPSJyZ2JhKDE5NCwxODIsMTA2LDAuMDMpIiBzdHJva2Utd2lkdGg9IjEiLz48L3BhdHRlcm4+PC9kZWZzPjxyZWN0IHdpZHRoPSIxMDAlIiBoZWlnaHQ9IjEwMCUiIGZpbGw9InVybCgjZ3JpZCkiLz48L3N2Zz4=')] opacity-20" />
      
      <div className="flex h-screen relative z-10">
        {/* Left Sidebar */}
        <div className={`flex flex-col border-r border-border bg-[var(--sidebar)] backdrop-blur-xl transition-all duration-300 z-50 relative ${sidebarCollapsed ? "w-16 items-center" : "w-80"}`}>
          {/* Logo & Actions Header */}
          <div className={`flex items-center p-4 border-b border-border h-16 w-full ${sidebarCollapsed ? "justify-center" : "justify-between"}`}>
            {sidebarCollapsed ? (
              <Waves className="size-6 text-primary hydro-glow-sm" />
            ) : (
              <>
                <div className="flex items-center gap-2">
                  <Waves className="size-6 text-primary hydro-glow-sm" />
                  <span className="font-bold text-lg text-gradient tracking-tight">Hydro</span>
                </div>
                
                <div className="flex items-center gap-1.5">
                  {/* Scripting Icon */}
                  <button
                    onClick={() => setMainView(mainView === "scripting" ? "sessions" : "scripting")}
                    className={`p-2 rounded-lg transition-all ${
                      mainView === "scripting"
                        ? "bg-primary/20 text-primary border border-primary/40 hydro-glow-sm"
                        : "hover:bg-white/5 text-muted-foreground hover:text-foreground border border-transparent hover:border-primary/20"
                    }`}
                    title="Lua Scripting"
                  >
                    <Code className="size-4" />
                  </button>
                  
                  {/* Settings Icon */}
                  <button
                    onClick={() => setSettingsOpen(true)}
                    className="p-2 rounded-lg transition-all hover:bg-white/5 text-muted-foreground hover:text-foreground border border-transparent hover:border-primary/20"
                    title="Settings"
                  >
                    <Gear className="size-4" />
                  </button>
                  
                  {/* Collapse Toggle */}
                  <button 
                    onClick={() => setSidebarCollapsed(!sidebarCollapsed)} 
                    className="p-2 rounded-lg transition-all hover:bg-white/5 text-muted-foreground hover:text-foreground border border-transparent hover:border-primary/20"
                    title="Collapse Sidebar"
                  >
                    <List className="size-4" />
                  </button>
                </div>
              </>
            )}
          </div>
          
          {/* Expand Button (Only visible when collapsed) */}
          {sidebarCollapsed && (
            <div className="w-full p-2 border-b border-border">
              <button 
                onClick={() => setSidebarCollapsed(false)} 
                className="w-full p-2 rounded-lg transition-all hover:bg-white/5 text-muted-foreground hover:text-foreground border border-transparent hover:border-primary/20"
                title="Expand Sidebar"
              >
                <CaretRight className="size-4" />
              </button>
            </div>
          )}

          {/* Create Session Section */}
          <div className="group relative w-full border-b border-border">
            <div 
              className={`flex items-center gap-3 p-4 cursor-pointer hover:bg-primary/5 transition-all ${sidebarCollapsed ? 'justify-center' : ''}`}
              onClick={() => {
                if (!sidebarCollapsed) {
                  setCreateSessionExpanded(!createSessionExpanded)
                }
              }}
            >
              <Plug className="size-5 text-primary shrink-0" />
              {!sidebarCollapsed && <h2 className="font-semibold text-sm truncate flex-1">Auth Session</h2>}
              {!sidebarCollapsed && (
                <CaretRight className={`size-4 text-muted-foreground transition-transform ${createSessionExpanded ? 'rotate-90' : ''}`} />
              )}
              {sidebarCollapsed && <CaretRight className="size-3 text-muted-foreground hidden group-hover:block absolute right-1" />}
            </div>
            
            <div className={`
              overflow-hidden transition-all duration-300 ease-in-out
              ${sidebarCollapsed 
                ? 'absolute left-[100%] top-0 w-80 bg-[var(--sidebar)] backdrop-blur-xl border border-border rounded-r-xl opacity-0 invisible group-hover:opacity-100 group-hover:visible shadow-2xl p-4 z-[60] ml-0.5' 
                : createSessionExpanded 
                  ? 'px-4 pb-4 block opacity-100 max-h-[1000px]' 
                  : 'px-4 max-h-0 opacity-0 invisible'
              }
            `}>
              <div className="space-y-4 rounded-xl border border-border bg-black/40 p-3 mt-2 shadow-inner">
              <div>
                <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">Auth Kind</label>
                <Select
                  value={authKind}
                  onValueChange={(value) => setAuthKind(value as AuthKind)}
                >
                  <SelectTrigger className="w-full rounded-lg border-border bg-black/60 h-8 text-xs focus:ring-primary/30 hydro-border-glow">
                    <SelectValue placeholder="Select Auth Method" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="jwt">JWT Token</SelectItem>
                    <SelectItem value="email_password">Email + Password</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {authKind !== "none" && (
                <>
                <div>
                  <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">Device ID</label>
                <div className="flex gap-2">
                  <Input
                    value={deviceId}
                    onChange={(event) => setDeviceId(event.target.value)}
                    placeholder="57ce..."
                    className="rounded-lg border-border bg-black/60 h-9 text-xs font-mono focus:border-primary/50 focus:ring-1 focus:ring-primary/30 transition-all"
                  />
                  <Button
                    variant="ghost"
                    size="sm"
                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary h-9 px-3 text-xs transition-all"
                    onClick={() => setDeviceId(generateDeviceId())}
                  >
                    Gen
                  </Button>
                </div>

                <div className="mt-3">
                  <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">Proxy / SOCKS5</label>
                  <div className="relative">
                    <Input
                      value={proxy}
                      onChange={(event) => setProxy(event.target.value)}
                      placeholder="http://user:pass@host:port"
                      className="rounded-lg border-border bg-black/60 h-9 text-xs focus:border-primary/50 focus:ring-1 focus:ring-primary/30 transition-all pl-8"
                    />
                    <Globe className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-primary/60" />
                  </div>
                  <p className="text-[9px] text-muted-foreground/60 italic mt-1">
                    Supports HTTP, HTTPS, and SOCKS5 (socks5://...)
                  </p>
                </div>
              </div>

              {authKind === "jwt" ? (
                <div>
                  <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">JWT Token</label>
                  <Textarea
                    value={jwt}
                    onChange={(event) => setJwt(event.target.value)}
                    rows={3}
                    placeholder="eyJ..."
                    className="rounded-lg border-border bg-black/60 text-xs font-mono focus:border-primary/50 focus:ring-1 focus:ring-primary/30 transition-all resize-none"
                  />
                </div>
              ) : null}

              {authKind === "email_password" ? (
                <>
                  <div>
                    <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">Email</label>
                    <Input
                      value={email}
                      onChange={(event) => setEmail(event.target.value)}
                      placeholder="you@example.com"
                      className="rounded-lg border-border bg-black/60 h-9 text-xs focus:border-primary/50 focus:ring-1 focus:ring-primary/30 transition-all"
                    />
                  </div>
                  <div>
                    <label className="text-[10px] text-muted-foreground mb-1 block uppercase tracking-wider">Password</label>
                    <Input
                      type="password"
                      value={password}
                      onChange={(event) => setPassword(event.target.value)}
                      className="rounded-lg border-border bg-black/60 h-9 text-xs focus:border-primary/50 focus:ring-1 focus:ring-primary/30 transition-all"
                    />
                  </div>
                </>
              ) : null}

              <Button
                onClick={() => void handleConnect()}
                disabled={connecting}
                className="w-full rounded-lg h-10 text-xs font-semibold border border-primary/40 bg-primary/10 hover:bg-primary/20 text-primary transition-all mt-2 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {connecting ? (
                  <>
                    <SpinnerGap className="size-4 animate-spin mr-2" />
                    Connecting...
                  </>
                ) : (
                  <>
                    <Plug className="size-4 mr-2" />
                    Connect Session
                  </>
                )}
              </Button>
              </>
              )}

              {feedback ? (
                <div
                  className={`rounded-lg border px-3 py-2 text-[10px] font-medium ${
                    feedback.kind === "error"
                      ? "hydro-error-banner"
                      : "border-[#6F8B57]/40 bg-[#6F8B57]/10 text-[#E7E4D8]"
                  }`}
                >
                  {feedback.message}
                </div>
              ) : null}
              </div>
            </div>
          </div>

          {/* Bot List Section */}
          <div className="group relative w-full border-b border-border flex-1 flex flex-col min-h-0">
            <div className={`flex items-center gap-3 p-4 cursor-pointer hover:bg-primary/5 transition-all ${sidebarCollapsed ? 'justify-center' : ''}`}>
              <Robot className="size-5 text-primary shrink-0" />
              {!sidebarCollapsed && <h2 className="font-semibold text-sm truncate flex-1">Bots</h2>}
              {sidebarCollapsed && <CaretRight className="size-3 text-muted-foreground hidden group-hover:block absolute right-1" />}
            </div>
            
            <div className={`
              overflow-y-auto transition-all duration-300 flex-1
              ${sidebarCollapsed 
                ? 'absolute left-[100%] top-0 w-80 h-96 max-h-[80vh] bg-[var(--sidebar)] backdrop-blur-xl border border-border rounded-r-xl opacity-0 invisible group-hover:opacity-100 group-hover:visible shadow-2xl p-4 z-[60] ml-0.5' 
                : 'px-4 pb-4 block'
              }
            `}>
              <div className="space-y-2">
                {sessions.map((session) => (
                  <div
                    key={session.id}
                    className={`rounded-lg border transition-all cursor-pointer ${
                      activeSessionId === session.id
                        ? "border-primary/60 bg-primary/10 hydro-border-glow"
                        : "border-border bg-black/40 hover:border-primary/30 hover:bg-primary/5"
                    }`}
                  >
                    <button
                      onClick={() => setActiveSessionId(session.id)}
                      className="w-full text-left p-2"
                    >
                      <div className="flex items-center gap-2 mb-1">
                        <span className="font-bold text-xs truncate flex-1">{session.id}</span>
                        <Badge
                          variant={statusVariant(session.status)}
                          className="rounded-full px-1.5 text-[8px] font-bold uppercase"
                        >
                          {session.status}
                        </Badge>
                      </div>
                      <div className="text-[10px] text-muted-foreground truncate">
                        {session.username ?? "No user"}
                      </div>
                    </button>
                    <div className="px-2 pb-2">
                      <Button
                        size="sm"
                        variant="outline"
                        className="w-full rounded-md border-red-500/40 bg-red-500/10 text-red-300 hover:bg-red-500/20 h-6 text-[10px]"
                        onClick={async () => {
                          if (confirm(`Delete session ${session.id}?`)) {
                            try {
                              await deleteSession(session.id)
                              setSessions((current) =>
                                current.filter((s) => s.id !== session.id)
                              )
                              setFeedback({
                                kind: "success",
                                message: `Session ${session.id} deleted`,
                              })
                            } catch (error) {
                              setFeedback({
                                kind: "error",
                                message:
                                  error instanceof Error
                                    ? error.message
                                    : "delete failed",
                              })
                            }
                          }
                        }}
                      >
                        Delete
                      </Button>
                    </div>
                  </div>
                ))}
                {sessions.length === 0 && (
                  <div className="text-xs text-muted-foreground text-center py-4">
                    No bots yet
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Main Content Area */}
        <div className="flex-1 overflow-y-auto">
          {mainView === "sessions" ? (
            <div className="mx-auto max-w-[1400px] p-6">
            {/* Header */}
            <div className="mb-8 relative rounded-3xl overflow-hidden glass border-white/10 p-8 shadow-2xl">
              <div className="absolute inset-0 bg-gradient-to-br from-primary/20 via-transparent to-transparent opacity-50"></div>
              <div className="absolute -top-10 -right-10 p-8 opacity-20 pointer-events-none">
                 <Moon className="size-64 text-primary blur-[60px]" />
              </div>
              <div className="relative z-10">
                <div className="flex items-center gap-4 mb-3">
                  <div className="p-3 bg-primary/20 rounded-2xl border border-primary/30 shadow-[0_0_20px_rgba(var(--primary),0.3)]">
                    <Moon className="size-8 text-primary" />
                  </div>
                  <h1 className="text-4xl font-extrabold tracking-tight text-white drop-shadow-md flex items-center gap-3">
                    Hydro <span className="text-primary drop-shadow-[0_0_10px_rgba(var(--primary),0.5)]">Dashboard</span>
                    <div className="flex gap-1 ml-2">
                      <ShieldCheck className="size-5 text-emerald-400/80 drop-shadow-[0_0_8px_rgba(52,211,153,0.4)]" />
                      <Globe className="size-5 text-sky-400/80 drop-shadow-[0_0_8px_rgba(56,189,248,0.4)]" />
                    </div>
                  </h1>
                </div>
                <p className="text-base text-white/70 max-w-xl leading-relaxed">
                  Welcome to your central command. Manage your bot sessions, configure automation parameters, and write powerful Lua scripts to dominate your world.
                </p>
                <div className="mt-6 flex flex-wrap items-center gap-4 text-xs font-mono">
                  <div className="flex items-center gap-2 bg-white/5 rounded-lg px-3 py-1.5 border border-white/10 backdrop-blur-sm">
                    <span className="relative flex h-2 w-2">
                      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
                    </span>
                    System Online
                  </div>
                  <div className="flex items-center gap-2 bg-white/5 rounded-lg px-3 py-1.5 border border-white/10 backdrop-blur-sm text-muted-foreground">
                    <Plug className="size-3" />
                    {sessions.length} Active Bot{sessions.length === 1 ? "" : "s"}
                  </div>
                </div>
              </div>
            </div>

            {/* Bot Session Details */}
            <div className="grid gap-4">
            {sessions.length ? (
              <Tabs
                value={activeSessionId}
                onValueChange={setActiveSessionId}
                className="gap-4"
              >
                {sessions.map((session) => {
                  if (activeSessionId !== session.id) {
                    return <TabsContent key={session.id} value={session.id} />
                  }
                  const inputs = sessionInputs[session.id] ?? EMPTY_INPUTS
                  const minimap = minimaps[session.id] ?? null
                    return (
                      <TabsContent key={session.id} value={session.id} className="mt-0">
                        <Card className="glass overflow-hidden border-white/10 shadow-2xl transition-all">
                        <CardHeader>
                          <div className="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-start sm:justify-between">
                            <div className="grid gap-1">
                              <CardTitle className="flex items-center gap-2">
                                <Plug className="size-4" />
                                {session.id}
                              </CardTitle>
                              <CardDescription>
                                user={session.username ?? "-"} world={session.current_world ?? "-"} host=
                                {session.current_host}:{session.current_port}
                              </CardDescription>
                              <div className="text-xs text-muted-foreground">
                                position map=({session.player_position.map_x ?? "-"},{" "}
                                {session.player_position.map_y ?? "-"}) world=(
                                {session.player_position.world_x ?? "-"},{" "}
                                {session.player_position.world_y ?? "-"}) ping=
                                {session.ping_ms != null ? `${session.ping_ms}ms` : "-"}
                              </div>
                              {session.last_error ? (
                                <div className="text-xs text-red-200">{session.last_error}</div>
                              ) : null}
                            </div>
                            <Badge
                              variant={statusVariant(session.status)}
                              className="rounded-full px-3"
                            >
                              {session.status}
                            </Badge>
                          </div>
                        </CardHeader>

                        <CardContent className="grid gap-4">
                          {/* Cheat Features - Organized by Category */}
                          <Tabs defaultValue="world" className="w-full">
                            <TabsList className="grid w-full grid-cols-3 rounded-xl border border-white/10 bg-black/40 p-1.5 shadow-inner">
                              <TabsTrigger value="world" className="rounded-lg data-[state=active]:bg-primary/20 data-[state=active]:text-primary data-[state=active]:shadow-[0_0_15px_-3px_rgba(var(--primary),0.3)] transition-all font-bold">
                                World
                              </TabsTrigger>
                              <TabsTrigger value="automation" className="rounded-lg data-[state=active]:bg-primary/20 data-[state=active]:text-primary data-[state=active]:shadow-[0_0_15px_-3px_rgba(var(--primary),0.3)] transition-all font-bold">
                                Automation
                              </TabsTrigger>
                              <TabsTrigger value="chat" className="rounded-lg data-[state=active]:bg-primary/20 data-[state=active]:text-primary data-[state=active]:shadow-[0_0_15px_-3px_rgba(var(--primary),0.3)] transition-all font-bold">
                                Chat
                              </TabsTrigger>
                            </TabsList>

                            {/* World & Navigation Tab */}
                            <TabsContent value="world" className="mt-4 space-y-3">
                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <NavigationArrow className="size-3.5" />
                                  World Navigation
                                </div>
                                <div className="grid gap-2 sm:grid-cols-[1fr_auto]">
                                  <Input
                                    value={inputs.world}
                                    onChange={(event) =>
                                      mutateSessionInput(session.id, "world", event.target.value)
                                    }
                                    placeholder="World name"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  />
                                  <Button
                                    className="rounded-xl"
                                    onClick={() =>
                                      void runAction(() => joinWorld(session.id, inputs.world))
                                    }
                                  >
                                    Join
                                  </Button>
                                </div>
                                <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() => void runAction(() => leaveWorld(session.id))}
                                  >
                                    Leave
                                  </Button>
                                  <Button
                                    variant="ghost"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() =>
                                      void runAction(() => disconnectSession(session.id))
                                    }
                                  >
                                    Disconnect
                                  </Button>
                                  <Button
                                    variant="ghost"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() =>
                                      void runAction(() => reconnectSession(session.id))
                                    }
                                  >
                                    Reconnect
                                  </Button>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <GameController className="size-3.5" />
                                  Movement Matrix
                                </div>
                                <div className="grid gap-2">
                                  <div className="grid grid-cols-3 gap-2">
                                    <div />
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() =>
                                        void runAction(() => moveSession(session.id, "up"))
                                      }
                                    >
                                      Up
                                    </Button>
                                    <div />
                                  </div>
                                  <div className="grid grid-cols-3 gap-2">
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() =>
                                        void runAction(() => moveSession(session.id, "left"))
                                      }
                                    >
                                      Left
                                    </Button>
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() =>
                                        void runAction(() => moveSession(session.id, "down"))
                                      }
                                    >
                                      Down
                                    </Button>
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() =>
                                        void runAction(() => moveSession(session.id, "right"))
                                      }
                                    >
                                      Right
                                    </Button>
                                  </div>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Waves className="size-3.5" />
                                  Minimap & Spatial Position
                                </div>
                                <MinimapPanel
                                  minimap={minimap}
                                  aiEnemies={session.ai_enemies}
                                  otherPlayers={session.other_players}
                                  playerPosition={session.player_position}
                                  currentWorld={session.current_world}
                                  currentTarget={session.current_target}
                                  collectables={session.collectables}
                                  onHoverChange={getHoverChange(session.id)}
                                />
                                <div className="text-xs text-muted-foreground">
                                  {hoverTiles[session.id] ?? "Hover a tile to inspect it."}
                                </div>
                              </div>
                            </TabsContent>

                            {/* Automation Tab */}
                            <TabsContent value="automation" className="mt-4 space-y-3">
                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Moon className="size-3.5" />
                                  Hydro Automine
                                </div>
                                <div className="grid grid-cols-2 gap-2">
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-emerald-500/40 bg-emerald-500/10 text-emerald-300 font-bold hover:bg-emerald-500/20 shadow-[0_0_15px_-3px_rgba(16,185,129,0.3)]"
                                    onClick={() => void runAction(() => startAutomine(session.id))}
                                  >
                                    Engage Automine
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-rose-500/40 bg-rose-500/10 text-rose-300 font-bold hover:bg-rose-500/20 shadow-[0_0_15px_-3px_rgba(244,63,94,0.3)]"
                                    onClick={() => void runAction(() => stopAutomine(session.id))}
                                  >
                                    Disengage
                                  </Button>
                                </div>
                              </div>

                              {/* Auto Clear */}
                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Moon className="size-3.5" />
                                  Auto Clear World
                                </div>
                                <div className="text-xs text-muted-foreground mb-2">
                                  Automatically clears the specified world top-down using pathfinding and smart mining logic.
                                </div>
                                <div className="grid gap-2 sm:grid-cols-[1fr_auto_auto]">
                                  <Input
                                    value={inputs.autoclearWorld}
                                    onChange={(event) =>
                                      mutateSessionInput(session.id, "autoclearWorld", event.target.value)
                                    }
                                    placeholder="World to clear"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  />
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-emerald-500/40 bg-emerald-500/10 text-emerald-300 font-bold hover:bg-emerald-500/20 shadow-[0_0_15px_-3px_rgba(16,185,129,0.3)]"
                                    onClick={() => void runAction(() => startAutoclear(session.id, inputs.autoclearWorld))}
                                  >
                                    Start Clear
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-rose-500/40 bg-rose-500/10 text-rose-300 font-bold hover:bg-rose-500/20 shadow-[0_0_15px_-3px_rgba(244,63,94,0.3)]"
                                    onClick={() => void runAction(() => stopAutoclear(session.id))}
                                  >
                                    Stop
                                  </Button>
                                </div>
                              </div>

                              {/* Automine Speed Control */}
                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Gear className="size-3.5" />
                                  Automine Speed Setting
                                </div>
                                <div className="text-xs text-muted-foreground">
                                  Adjust mining speed. Lower = safer from anti-cheat, Higher = faster but riskier.
                                </div>
                                <div className="space-y-3">
                                  <div className="flex items-center justify-between">
                                    <span className="text-xs text-muted-foreground">Slower (Safe)</span>
                                    <span className="text-sm font-bold text-primary">
                                      {((automineSpeed[session.id] ?? 1.0) * 100).toFixed(0)}%
                                    </span>
                                    <span className="text-xs text-muted-foreground">Faster (Risk)</span>
                                  </div>
                                  <input
                                    type="range"
                                    min="0.4"
                                    max="1.6"
                                    step="0.1"
                                    value={automineSpeed[session.id] ?? 1.0}
                                    onChange={(e) => {
                                      const val = parseFloat(e.target.value)
                                      setAutomineSpeedState(prev => ({ ...prev, [session.id]: val }))
                                    }}
                                    className="w-full h-2 rounded-full appearance-none cursor-pointer accent-primary"
                                  />
                                  <div className="flex gap-1.5">
                                    {([0.5, 0.7, 1.0, 1.3, 1.6] as const).map(preset => (
                                      <Button
                                        key={preset}
                                        size="xs"
                                        variant="outline"
                                        className={`flex-1 rounded-lg text-[10px] border ${
                                          Math.abs((automineSpeed[session.id] ?? 1.0) - preset) < 0.05
                                            ? 'border-primary/60 bg-primary/20 text-primary'
                                            : 'border-border bg-black/40 text-muted-foreground hover:border-primary/40 hover:bg-primary/10'
                                        } transition-all`}
                                        onClick={() => setAutomineSpeedState(prev => ({ ...prev, [session.id]: preset }))}
                                      >
                                        {preset === 0.5 ? 'Safe' : preset === 1.0 ? 'Normal' : preset === 1.6 ? 'Max' : `${preset}x`}
                                      </Button>
                                    ))}
                                  </div>
                                  <Button
                                    className="w-full rounded-xl bg-primary/20 hover:bg-primary/30 border border-primary/50 text-primary font-bold"
                                    onClick={() => void runAction(() => setAutomineSpeed(session.id, automineSpeed[session.id] ?? 1.0))}
                                  >
                                    Apply Speed
                                  </Button>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Fish className="size-3.5" />
                                  Automated Fishing <Badge variant="destructive" className="ml-2 text-[8px]">MAINTENANCE</Badge>
                                </div>
                                <Input
                                  value={inputs.bait}
                                  onChange={(event) =>
                                    mutateSessionInput(session.id, "bait", event.target.value)
                                  }
                                  placeholder="Bait name or lure name"
                                  className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                />
                                <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() =>
                                      void runAction(() =>
                                        startFishing(session.id, "left", inputs.bait),
                                      )
                                    }
                                    disabled
                                  >
                                    Fish Left
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() =>
                                      void runAction(() =>
                                        startFishing(session.id, "right", inputs.bait),
                                      )
                                    }
                                    disabled
                                  >
                                    Fish Right
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() => void runAction(() => stopFishing(session.id))}
                                    disabled
                                  >
                                    Stop
                                  </Button>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Waves className="size-3.5" />
                                  Autonether (Nether Farming) <Badge variant="destructive" className="ml-2 text-[8px]">MAINTENANCE</Badge>
                                </div>
                                <div className="text-xs text-muted-foreground mb-2">
                                  Automatically enters NETHERWORLD, collects Nether Keys, and exits through portal. Requires Nether Scroll in inventory.
                                </div>
                                <div className="grid gap-1 rounded-xl border border-white/10 bg-white/4 p-3 text-xs text-muted-foreground">
                                  <div>
                                    Status:{" "}
                                    <span
                                      className={
                                        autonetherStatuses[session.id]?.active
                                          ? "text-emerald-300"
                                          : "text-slate-200"
                                      }
                                    >
                                      {autonetherStatuses[session.id]?.active ? "running" : "idle"}
                                    </span>
                                  </div>
                                  <div>
                                    Phase: {autonetherStatuses[session.id]?.phase ?? "idle"}
                                  </div>
                                </div>
                                <div className="grid grid-cols-2 gap-2">
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-purple-500/40 bg-purple-500/10 text-purple-300 font-bold hover:bg-purple-500/20 shadow-[0_0_15px_-3px_rgba(168,85,247,0.3)]"
                                    onClick={() => void runAction(() => startAutonether(session.id))}
                                    disabled
                                  >
                                    Start Autonether
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-xl border-rose-500/40 bg-rose-500/10 text-rose-300 font-bold hover:bg-rose-500/20 shadow-[0_0_15px_-3px_rgba(244,63,94,0.3)]"
                                    onClick={() => void runAction(() => stopAutonether(session.id))}
                                    disabled
                                  >
                                    Stop
                                  </Button>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <Moon className="size-3.5" />
                                  Auto Tutorial <Badge variant="destructive" className="ml-2 text-[8px]">MAINTENANCE</Badge>
                                </div>
                                <div className="text-xs text-muted-foreground mb-2">
                                  Automatically finishes the Growtopia tutorial. Currently disabled for maintenance.
                                </div>
                                <Button
                                  variant="outline"
                                  className="w-full rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  onClick={() => void runAction(() => automateTutorial(session.id))}
                                  disabled
                                >
                                  Run Auto Tutorial
                                </Button>
                              </div>
                            </TabsContent>

                            {/* Chat Tab */}
                            <TabsContent value="chat" className="mt-4 space-y-3">
                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <ChatCenteredDots className="size-3.5" />
                                  Chat Messages
                                </div>
                                <div className="grid gap-2">
                                  <Input
                                    value={inputs.chat}
                                    onChange={(event) =>
                                      mutateSessionInput(session.id, "chat", event.target.value)
                                    }
                                    placeholder="Say something in world chat"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  />
                                  <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() => void runAction(() => talk(session.id, inputs.chat))}
                                    >
                                      Send Chat
                                    </Button>
                                    <Button
                                      variant="outline"
                                      className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                      onClick={() => mutateSessionInput(session.id, "chat", "")}
                                    >
                                      Clear
                                    </Button>
                                  </div>
                                </div>
                              </div>

                              <div className="grid gap-3 rounded-2xl glass-dark p-4">
                                <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                  <ChatCenteredDots className="size-3.5" />
                                  Auto Spam
                                </div>
                                <div className="text-xs text-muted-foreground">
                                  Repeats the same world chat message with your chosen delay.
                                </div>
                                <div className="grid gap-2 sm:grid-cols-[1fr_120px]">
                                  <Input
                                    value={inputs.spam}
                                    onChange={(event) =>
                                      mutateSessionInput(session.id, "spam", event.target.value)
                                    }
                                    placeholder="Spam message"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  />
                                  <Input
                                    type="number"
                                    min="0.25"
                                    step="0.25"
                                    value={inputs.spamDelay}
                                    onChange={(event) =>
                                      mutateSessionInput(session.id, "spamDelay", event.target.value)
                                    }
                                    placeholder="5"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                  />
                                </div>
                                <div className="text-[11px] text-muted-foreground">
                                  Delay in seconds.
                                </div>
                                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() =>
                                      void runAction(() =>
                                        startSpam(
                                          session.id,
                                          inputs.spam,
                                          parseSpamDelayMs(inputs.spamDelay),
                                        ),
                                      )
                                    }
                                  >
                                    Start Spam
                                  </Button>
                                  <Button
                                    variant="outline"
                                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                                    onClick={() => void runAction(() => stopSpam(session.id))}
                                  >
                                    Stop Spam
                                  </Button>
                                </div>
                              </div>
                            </TabsContent>
                          </Tabs>

                          {/* Inventory - Always Visible */}
                          <div className="grid gap-3">
                              <div className="flex items-center gap-2 text-[10px] font-bold uppercase tracking-widest text-primary/70">
                                <Gear className="size-3.5" />
                                Inventory Storage
                              </div>
                              <div className="flex flex-wrap gap-2">
                              {session.inventory.length ? (
                                session.inventory.map((item) => {
                                  const blockName = blockNames[String(item.block_id)]
                                  const label =
                                    blockName && blockName !== "None"
                                      ? camelToWords(blockName)
                                      : `#${item.block_id}`
                                  return (
                                    <div
                                      key={`${session.id}-${item.block_id}-${item.inventory_type}`}
                                      className="grid min-w-[92px] flex-1 gap-1 rounded-xl border border-white/10 bg-white/4 p-2.5 text-center sm:min-w-[110px] sm:flex-none sm:p-3"
                                    >
                                      <TileSprite
                                        blockId={item.block_id}
                                        size={40}
                                        className="mx-auto"
                                        fallback={<span className="text-[9px] text-muted-foreground">?</span>}
                                      />
                                      <div className="font-mono text-[11px] text-muted-foreground">
                                        #{item.block_id}
                                      </div>
                                      <div className="text-xs font-medium">{label}</div>
                                      <div className="text-lg font-semibold text-cyan-300">
                                        x{item.amount}
                                      </div>
                                      <div className="text-[11px] text-muted-foreground">
                                        {getItemCategory(blockName, inventoryTypeLabel(item.inventory_type))}
                                      </div>
                                      {CLOTHING_TYPES.has(item.inventory_type) ? (
                                        <div className="grid grid-cols-2 gap-2">
                                          <Button
                                            size="xs"
                                            variant="outline"
                                            className="rounded-lg border-white/10 bg-white/5"
                                            onClick={() =>
                                              void runAction(() =>
                                                wearItem(session.id, item.block_id, true),
                                              )
                                            }
                                          >
                                            Wear
                                          </Button>
                                          <Button
                                            size="xs"
                                            variant="outline"
                                            className="rounded-lg border-white/10 bg-white/5"
                                            onClick={() =>
                                              void runAction(() =>
                                                wearItem(session.id, item.block_id, false),
                                              )
                                            }
                                          >
                                            Off
                                          </Button>
                                        </div>
                                      ) : null}
                                      <div className="flex gap-1">
                                        <Input
                                          className="h-6 min-w-0 flex-1 rounded-lg border-white/10 bg-white/5 px-1.5 text-center text-xs"
                                          type="number"
                                          min={1}
                                          max={item.amount}
                                          value={dropAmounts[`${session.id}-${item.block_id}-${item.inventory_type}`] ?? "1"}
                                          onChange={(e) => {
                                            const key = `${session.id}-${item.block_id}-${item.inventory_type}`
                                            setDropAmounts((prev) => ({ ...prev, [key]: e.target.value }))
                                          }}
                                        />
                                        <Button
                                          size="xs"
                                          variant="outline"
                                          className="rounded-lg border-white/10 bg-white/5"
                                          onClick={() => {
                                            const key = `${session.id}-${item.block_id}-${item.inventory_type}`
                                            const amt = Math.max(1, Math.min(item.amount, parseInt(dropAmounts[key] ?? "1", 10) || 1))
                                            void runAction(() =>
                                              dropItem(session.id, item.block_id, item.inventory_type, amt),
                                            )
                                          }}
                                        >
                                          Drop
                                        </Button>
                                      </div>
                                    </div>
                                  )
                                })
                              ) : (
                                <div className="rounded-xl border border-dashed border-white/10 px-4 py-5 text-xs text-muted-foreground">
                                  Inventory empty.
                                </div>
                              )}
                            </div>
                          </div>
                        </CardContent>
                      </Card>
                    </TabsContent>
                  )
                })}
              </Tabs>
            ) : (
              <Card className="border-white/10 bg-card/90 ring-white/10">
                <CardContent className="px-4 py-8 text-center text-sm text-muted-foreground">
                  No sessions yet. Create a bot using the connection form above.
                </CardContent>
              </Card>
            )}
          </div>
        </div>
      ) : mainView === "scripting" ? (
        <div className="flex h-full bg-black/40">
          {/* Main Editor Area */}
          <div className="flex-1 flex flex-col p-6">
            <div className="flex items-center justify-between mb-4 pb-4 border-b border-white/10">
              <div className="flex items-center gap-3">
                <Code className="size-6 text-primary hydro-glow-sm" />
                <div>
                  <h1 className="text-xl font-bold text-gradient">Lua Script Editor</h1>
                  <p className="text-xs text-muted-foreground">Session: {activeSessionId ? sessions.find(s => s.id === activeSessionId)?.username || activeSessionId : "None"}</p>
                </div>
              </div>
              <Button 
                variant="ghost" 
                className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all" 
                onClick={() => setMainView("sessions")}
              >
                Close Editor
              </Button>
            </div>
            
            {activeSessionId && sessions.find(s => s.id === activeSessionId) ? (
              <div className="flex-1 flex flex-col gap-4">
                <Textarea
                  value={sessionInputs[activeSessionId]?.luaSource ?? ""}
                  onChange={(event) =>
                    mutateSessionInput(activeSessionId, "luaSource", event.target.value)
                  }
                  className="flex-1 rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-foreground transition-all font-mono text-sm p-4 focus-visible:ring-primary/50 resize-none"
                  placeholder="-- Write your Lua script here\nbot:talk('Hello World!')\nbot:sleep(1000)"
                />
                <div className="flex flex-wrap items-center gap-2">
                  <Button 
                    className="rounded-xl bg-primary/20 hover:bg-primary/30 border border-primary/50 text-primary hydro-glow-sm"
                    onClick={() => void runLuaAction(activeSessionId, () => startLuaScript(activeSessionId, sessionInputs[activeSessionId]?.luaSource ?? ""))}
                    disabled={luaStatuses[activeSessionId]?.running}
                  >
                    <Bug className="size-4 mr-2" />
                    Execute Script
                  </Button>
                  <Button 
                    variant="outline" 
                    className="rounded-xl border-rose-500/40 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20"
                    onClick={() => void runAction(() => stopLuaScript(activeSessionId))}
                    disabled={!luaStatuses[activeSessionId]?.running}
                  >
                    Stop Script
                  </Button>
                  <Button 
                    variant="outline" 
                    className="rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all"
                    onClick={() => void refreshLuaStatus(activeSessionId)}
                  >
                    Refresh Status
                  </Button>
                  
                  <div className="ml-auto flex items-center gap-3">
                    {luaStatuses[activeSessionId]?.running && (
                      <SpinnerGap className="size-4 animate-spin text-primary" />
                    )}
                    <div className="text-xs font-mono px-3 py-1.5 rounded-lg border border-white/10 bg-white/5">
                      <span className={luaStatuses[activeSessionId]?.running ? "text-emerald-400" : "text-slate-400"}>
                        {luaStatuses[activeSessionId]?.running ? "● Running" : "○ Idle"}
                      </span>
                    </div>
                  </div>
                </div>
                
                {/* Status Messages */}
                {luaStatuses[activeSessionId]?.last_error && (
                  <div className="rounded-lg border border-rose-500/40 bg-rose-500/10 p-3 text-sm text-rose-300">
                    <div className="font-bold mb-1">Error:</div>
                    <div className="font-mono text-xs">{luaStatuses[activeSessionId]?.last_error}</div>
                  </div>
                )}
                {luaStatuses[activeSessionId]?.last_result_message && !luaStatuses[activeSessionId]?.last_error && (
                  <div className="rounded-lg border border-emerald-500/40 bg-emerald-500/10 p-3 text-sm text-emerald-300">
                    <div className="font-bold mb-1">Success:</div>
                    <div className="font-mono text-xs">{luaStatuses[activeSessionId]?.last_result_message}</div>
                  </div>
                )}
              </div>
            ) : (
              <div className="flex-1 flex items-center justify-center">
                <Card className="border-white/10 bg-card/50 ring-white/10 max-w-md w-full">
                  <CardContent className="px-4 py-8 text-center flex flex-col items-center gap-3">
                    <Robot className="size-12 text-muted-foreground opacity-50" />
                    <div className="text-sm text-muted-foreground">
                      Please select an active bot session from the sidebar to edit its script.
                    </div>
                  </CardContent>
                </Card>
              </div>
            )}
          </div>

          {/* Documentation Sidebar */}
          <div className="w-96 border-l border-border bg-black/60 backdrop-blur-xl overflow-y-auto p-4">
            <h2 className="text-lg font-bold mb-4 text-primary">API Documentation</h2>
            
            <div className="space-y-4 text-xs">
              {/* Movement Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🚶 Movement</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:walk(dx, dy)</code> - Walk relative</div>
                  <div><code className="text-emerald-400">bot:walkTo(x, y)</code> - Walk to position</div>
                  <div><code className="text-emerald-400">bot:walkToSpawn()</code> - Walk to spawn</div>
                  <div><code className="text-emerald-400">bot:findPath(x, y)</code> - Get path array</div>
                </div>
              </div>

              {/* World Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🌍 World</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:warp(world)</code> - Warp to world</div>
                  <div><code className="text-emerald-400">bot:getCurrentWorld()</code> - Get world name</div>
                  <div><code className="text-emerald-400">bot:getWorld()</code> - Get world data</div>
                  <div><code className="text-emerald-400">bot:getWorldSize()</code> - Get width/height</div>
                </div>
              </div>

              {/* Block Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🧱 Blocks</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:punch(dx, dy)</code> - Punch relative</div>
                  <div><code className="text-emerald-400">bot:punchAt(x, y)</code> - Punch at position</div>
                  <div><code className="text-emerald-400">bot:place(dx, dy, id)</code> - Place relative</div>
                  <div><code className="text-emerald-400">bot:placeAt(x, y, id)</code> - Place at position</div>
                  <div><code className="text-emerald-400">bot:getTileAt(x, y)</code> - Get tile info</div>
                  <div><code className="text-emerald-400">bot:isSolid(x, y)</code> - Check if solid</div>
                  <div><code className="text-emerald-400">bot:isEmpty(x, y)</code> - Check if empty</div>
                  <div><code className="text-emerald-400">bot:findBlock(id, dist)</code> - Find nearest block</div>
                  <div><code className="text-emerald-400">bot:countBlocks(id)</code> - Count blocks</div>
                </div>
              </div>

              {/* Inventory Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🎒 Inventory</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:getInventory()</code> - Get all items</div>
                  <div><code className="text-emerald-400">bot:getInventoryCount(id)</code> - Get item count</div>
                  <div><code className="text-emerald-400">bot:hasItem(id, min)</code> - Check if has item</div>
                  <div><code className="text-emerald-400">bot:wear(id)</code> - Wear item</div>
                  <div><code className="text-emerald-400">bot:unwear(id)</code> - Unwear item</div>
                </div>
              </div>

              {/* Farming Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🌾 Farming</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:isTileReadyToHarvest(x, y)</code> - Check ready</div>
                  <div><code className="text-emerald-400">bot:harvestAll()</code> - Harvest all ready tiles</div>
                </div>
              </div>

              {/* Collection Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">💎 Collection</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:collect()</code> - Collect items</div>
                  <div><code className="text-emerald-400">bot:getCollectables()</code> - Get all collectables</div>
                  <div><code className="text-emerald-400">bot:findNearestCollectable()</code> - Find nearest</div>
                </div>
              </div>

              {/* Utility Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🔧 Utility</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:sleep(ms)</code> - Sleep milliseconds</div>
                  <div><code className="text-emerald-400">bot:talk(msg)</code> - Send chat message</div>
                  <div><code className="text-emerald-400">bot:log(msg)</code> - Print to console</div>
                  <div><code className="text-emerald-400">bot:getPosition()</code> - Get current position</div>
                  <div><code className="text-emerald-400">bot:getStatus()</code> - Get bot status</div>
                  <div><code className="text-emerald-400">bot:isInWorld()</code> - Check if in world</div>
                  <div><code className="text-emerald-400">bot:isNearSpawn(range)</code> - Check near spawn</div>
                </div>
              </div>

              {/* Math Functions */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">📐 Math</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:getDistance(x1,y1,x2,y2)</code> - Euclidean</div>
                  <div><code className="text-emerald-400">bot:getManhattanDistance(x1,y1,x2,y2)</code> - Manhattan</div>
                  <div><code className="text-emerald-400">bot:isInRange(x1,y1,x2,y2,r)</code> - Check range</div>
                </div>
              </div>

              {/* Control Flow */}
              <div className="rounded-lg border border-border bg-black/40 p-3">
                <h3 className="font-bold text-primary mb-2">🔄 Control Flow</h3>
                <div className="space-y-2 text-muted-foreground">
                  <div><code className="text-emerald-400">bot:repeat(n, func)</code> - Repeat function</div>
                  <div><code className="text-emerald-400">bot:waitUntil(cond, timeout)</code> - Wait for condition</div>
                </div>
              </div>

              {/* Examples */}
              <div className="rounded-lg border border-primary/40 bg-primary/10 p-3">
                <h3 className="font-bold text-primary mb-2">📝 Examples</h3>
                <div className="space-y-3 text-muted-foreground">
                  <div>
                    <div className="font-bold text-xs mb-1">Simple Walk:</div>
                    <pre className="text-[10px] bg-black/60 p-2 rounded overflow-x-auto">
{`bot:walkTo(50, 50)
bot:talk("I'm here!")
bot:sleep(1000)`}
                    </pre>
                  </div>
                  <div>
                    <div className="font-bold text-xs mb-1">Find & Break Block:</div>
                    <pre className="text-[10px] bg-black/60 p-2 rounded overflow-x-auto">
{`local pos = bot:findBlock(2, 100)
if pos then
  bot:walkTo(pos.x, pos.y)
  bot:punchAt(pos.x, pos.y)
end`}
                    </pre>
                  </div>
                  <div>
                    <div className="font-bold text-xs mb-1">Harvest Loop:</div>
                    <pre className="text-[10px] bg-black/60 p-2 rounded overflow-x-auto">
{`while true do
  bot:harvestAll()
  bot:collect()
  bot:sleep(5000)
end`}
                    </pre>
                  </div>
                  <div>
                    <div className="font-bold text-xs mb-1">Check Inventory:</div>
                    <pre className="text-[10px] bg-black/60 p-2 rounded overflow-x-auto">
{`if bot:hasItem(2, 10) then
  bot:talk("I have 10+ dirt!")
else
  bot:talk("Need more dirt")
end`}
                    </pre>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      ) : null}
      </div>
    </div>

      <Dialog
        open={tutorialCompleted !== null}
        onOpenChange={(open) => {
          if (!open) {
            setTutorialCompleted(null)
          }
        }}
      >
        <DialogContent className="rounded-2xl border-white/10 bg-card text-sm">
          <DialogHeader>
            <DialogTitle>Tutorial Finished</DialogTitle>
            <DialogDescription>
              {tutorialCompleted?.message ?? "Tutorial finished."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter showCloseButton />
        </DialogContent>
      </Dialog>

      <Dialog open={settingsOpen} onOpenChange={setSettingsOpen}>
        <DialogContent className="rounded-2xl border-border bg-card text-foreground backdrop-blur-xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Gear className="size-5 text-primary hydro-glow-sm" />
              Dashboard Settings
            </DialogTitle>
            <DialogDescription>
              Manage your bot configurations and account security.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 py-4">
            {/* Code Account */}
            <Button 
              variant="ghost" 
              className="justify-start rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all h-11"
              onClick={() => {
                setSettingsOpen(false)
                // Navigate to login/code page by clearing auth
                setDashboardStatus(prev => prev ? { ...prev, authenticated: false } : null)
              }}
            >
              <Code className="size-4 mr-2" /> Code Account
            </Button>
            {/* Load Bot */}
            <Button 
              variant="ghost" 
              className="justify-start rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all h-11"
              onClick={() => {
                try {
                  const saved = localStorage.getItem('hydro_bot_config')
                  if (!saved) {
                    setFeedback({ kind: 'error', message: 'No saved bot config found.' })
                    return
                  }
                  const config = JSON.parse(saved) as { authKind?: string; deviceId?: string; jwt?: string; email?: string }
                  if (config.authKind) setAuthKind(config.authKind as AuthKind)
                  if (config.deviceId) setDeviceId(config.deviceId)
                  if (config.jwt) setJwt(config.jwt)
                  if (config.email) setEmail(config.email)
                  setFeedback({ kind: 'success', message: 'Bot config loaded!' })
                  setSettingsOpen(false)
                } catch {
                  setFeedback({ kind: 'error', message: 'Failed to load bot config.' })
                }
              }}
            >
              <Plug className="size-4 mr-2" /> Load Bot
            </Button>
            {/* Save Bot */}
            <Button 
              variant="ghost" 
              className="justify-start rounded-lg border border-border bg-black/40 hover:bg-primary/10 hover:border-primary/40 text-muted-foreground hover:text-primary transition-all h-11"
              onClick={() => {
                try {
                  const config = { authKind, deviceId, jwt, email }
                  localStorage.setItem('hydro_bot_config', JSON.stringify(config))
                  setFeedback({ kind: 'success', message: 'Bot config saved!' })
                  setSettingsOpen(false)
                } catch {
                  setFeedback({ kind: 'error', message: 'Failed to save bot config.' })
                }
              }}
            >
              <LockKey className="size-4 mr-2" /> Save Bot
            </Button>
            <div className="h-px bg-white/10 my-2" />
            <Button
              variant="outline"
              onClick={() => {
                setSettingsOpen(false)
                void handleDashboardLogout()
              }}
              disabled={dashboardBusy}
              className="justify-start border-rose-500/40 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20 hover:border-rose-500/60 font-bold shadow-[0_0_15px_-3px_rgba(244,63,94,0.2)]"
            >
              {dashboardBusy ? <SpinnerGap className="size-4 animate-spin mr-2" /> : <LockKey className="size-4 mr-2" />}
              Lock Dashboard
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  )
}

export default App
