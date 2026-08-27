import React, { useEffect, useState } from 'react'
import { Landing, ProjectStart } from './components/Landing'
import { Workspace } from './components/Workspace'
import { LanguageProvider, useLanguage } from './i18n'

const validStates = new Set(['ready', 'preflight', 'running', 'success', 'error'])

function readRoute() {
  const params = new URLSearchParams(window.location.search)
  const screen = params.get('screen') ?? 'landing'
  const requestedState = params.get('state') ?? 'ready'
  const requestedLanguage = params.get('lang')
  return {
    screen: ['landing', 'start', 'workspace'].includes(screen) ? screen : 'landing',
    state: validStates.has(requestedState) ? requestedState : 'ready',
    language: requestedLanguage === 'ko' ? 'ko' : 'en',
    // Distinguishes "the URL asked for English" from "the URL said nothing",
    // so a remembered choice is only overridden by an explicit request.
    languageRequested: requestedLanguage !== null,
  }
}

function writeRoute(screen, state = 'ready', replace = false, language = 'en') {
  const params = new URLSearchParams()
  if (screen !== 'landing') params.set('screen', screen)
  if (screen === 'workspace' && state !== 'ready') params.set('state', state)
  if (language === 'ko') params.set('lang', 'ko')
  const url = `${window.location.pathname}${params.size ? `?${params}` : ''}`
  window.history[replace ? 'replaceState' : 'pushState']({}, '', url)
}

function AppRoutes() {
  const [route, setRoute] = useState(readRoute)
  // Language now lives in one place for every screen instead of being a prop the
  // workspace route used to drop on the floor.
  const { language, setLanguage } = useLanguage()

  useEffect(() => {
    const sync = () => setRoute(readRoute())
    window.addEventListener('popstate', sync)
    return () => window.removeEventListener('popstate', sync)
  }, [])

  useEffect(() => {
    document.title =
      route.screen === 'landing'
        ? 'Xazz · Landing'
        : route.screen === 'start'
          ? `Xazz · Project Start${language === 'ko' ? ' · KO' : ''}`
          : `Xazz · Workspace · ${route.state}`
  }, [route, language])

  // Keep ?lang= in step with the chosen language so a shared link opens the same way.
  useEffect(() => {
    writeRoute(route.screen, route.state, true, language)
  }, [language, route.screen, route.state])

  const navigate = (screen, state = 'ready') => {
    writeRoute(screen, state, false, language)
    setRoute((current) => ({ ...current, screen, state }))
    window.scrollTo({ top: 0, behavior: 'instant' })
  }

  if (route.screen === 'start') {
    return (
      <ProjectStart
        onBack={() => navigate('landing')}
        onOpenWorkspace={() => navigate('workspace', 'ready')}
        language={language}
        onLanguageChange={setLanguage}
      />
    )
  }

  if (route.screen === 'workspace') {
    return (
      <Workspace
        initialState={route.state}
        onHome={() => navigate('landing')}
        onStateChange={(state) => {
          writeRoute('workspace', state, true, language)
          setRoute((current) => ({ ...current, screen: 'workspace', state }))
        }}
      />
    )
  }

  return <Landing onOpenSample={() => navigate('start')} />
}

export function App() {
  const initial = readRoute()
  return (
    <LanguageProvider
      initialLanguage={initial.language}
      forceInitial={initial.languageRequested}
    >
      <AppRoutes />
    </LanguageProvider>
  )
}
