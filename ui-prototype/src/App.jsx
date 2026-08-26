import React, { useEffect, useState } from 'react'
import { Landing, ProjectStart } from './components/Landing'
import { Workspace } from './components/Workspace'

const validStates = new Set(['ready', 'preflight', 'running', 'success', 'error'])

function readRoute() {
  const params = new URLSearchParams(window.location.search)
  const screen = params.get('screen') ?? 'landing'
  const requestedState = params.get('state') ?? 'ready'
  const requestedLanguage = params.get('lang') ?? 'en'
  return {
    screen: ['landing', 'start', 'workspace'].includes(screen) ? screen : 'landing',
    state: validStates.has(requestedState) ? requestedState : 'ready',
    language: requestedLanguage === 'ko' ? 'ko' : 'en',
  }
}

function writeRoute(screen, state = 'ready', replace = false, language = 'en') {
  const params = new URLSearchParams()
  if (screen !== 'landing') params.set('screen', screen)
  if (screen === 'workspace' && state !== 'ready') params.set('state', state)
  if (screen === 'start' && language === 'ko') params.set('lang', 'ko')
  const url = `${window.location.pathname}${params.size ? `?${params}` : ''}`
  window.history[replace ? 'replaceState' : 'pushState']({}, '', url)
}

export function App() {
  const [route, setRoute] = useState(readRoute)

  useEffect(() => {
    const sync = () => setRoute(readRoute())
    window.addEventListener('popstate', sync)
    return () => window.removeEventListener('popstate', sync)
  }, [])

  useEffect(() => {
    const title =
      route.screen === 'landing'
        ? 'Xazz · Landing'
        : route.screen === 'start'
          ? `Xazz · Project Start${route.language === 'ko' ? ' · KO' : ''}`
          : `Xazz · Workspace · ${route.state}`
    document.title = title
    document.documentElement.lang = route.language
  }, [route])

  const navigate = (screen, state = 'ready', language = 'en') => {
    writeRoute(screen, state, false, language)
    setRoute({ screen, state, language })
    window.scrollTo({ top: 0, behavior: 'instant' })
  }

  if (route.screen === 'start') {
    return (
      <ProjectStart
        onBack={() => navigate('landing')}
        onOpenWorkspace={() => navigate('workspace', 'ready')}
        language={route.language}
        onLanguageChange={(language) => {
          writeRoute('start', 'ready', true, language)
          setRoute({ screen: 'start', state: 'ready', language })
        }}
      />
    )
  }

  if (route.screen === 'workspace') {
    return (
      <Workspace
        initialState={route.state}
        onHome={() => navigate('landing')}
        onStateChange={(state) => {
          writeRoute('workspace', state, true)
          setRoute({ screen: 'workspace', state, language: 'en' })
        }}
      />
    )
  }

  return <Landing onOpenSample={() => navigate('start')} />
}
