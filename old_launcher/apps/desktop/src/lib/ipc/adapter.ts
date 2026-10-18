import {
  buildDefaultAccounts,
  buildInstancesForPreset,
  buildModsForPreset,
  DISCOVER_CATALOG,
  TYPICAL_CRASH_SIGNATURES,
  TYPICAL_JAVA_RUNTIMES,
  TYPICAL_SERVERS,
  TYPICAL_WORLDS,
  type AccountRecord,
  type CrashSignatureRecord,
  type DiscoverItemRecord,
  type FixturePreset,
  type InstanceRecord,
  type JavaRuntimeRecord,
  type ModRecord,
  type ServerRecord,
  type WorldRecord,
} from './fixtures';
import { computeOfflinePlayerUuid } from './offlineUuid';

export type AppView =
  | 'onboarding'
  | 'home'
  | 'library'
  | 'instance'
  | 'discover'
  | 'accounts'
  | 'java'
  | 'crash'
  | 'settings'
  | 'gallery';

export type ThemeMode = 'dark' | 'light' | 'high-contrast';

export interface InitialRouteConfig {
  fixture: FixturePreset;
  theme: ThemeMode;
  view: AppView;
  performanceMode: boolean;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    };
  }
}

export function parseInitialRouteConfig(): InitialRouteConfig {
  if (typeof window === 'undefined') {
    return { fixture: 'typical', theme: 'dark', view: 'home', performanceMode: false };
  }
  const params = new URLSearchParams(window.location.search);
  const path = window.location.pathname;

  const rawFixture = params.get('fixture');
  const fixture: FixturePreset =
    rawFixture === 'empty' || rawFixture === 'stress' || rawFixture === 'typical'
      ? rawFixture
      : 'typical';

  const rawTheme = params.get('theme');
  const theme: ThemeMode =
    rawTheme === 'light' || rawTheme === 'high-contrast' || rawTheme === 'dark'
      ? rawTheme
      : 'dark';

  let view: AppView = fixture === 'empty' ? 'onboarding' : 'home';
  if (path === '/__gallery' || params.get('view') === 'gallery') {
    view = 'gallery';
  } else {
    const requestedView = params.get('view') as AppView | null;
    if (
      requestedView &&
      [
        'onboarding',
        'home',
        'library',
        'instance',
        'discover',
        'accounts',
        'java',
        'crash',
        'settings',
        'gallery',
      ].includes(requestedView)
    ) {
      view = requestedView;
    }
  }

  const performanceMode = params.get('perf') === '1';

  return { fixture, theme, view, performanceMode };
}

export interface KestryxIpcSnapshot {
  runtimeMode: 'tauri-native' | 'mock-browser';
  fixture: FixturePreset;
  instances: InstanceRecord[];
  mods: ModRecord[];
  worlds: WorldRecord[];
  servers: ServerRecord[];
  accounts: AccountRecord[];
  javaRuntimes: JavaRuntimeRecord[];
  crashes: CrashSignatureRecord[];
  discoverCatalog: DiscoverItemRecord[];
}

export class DualModeIpcAdapter {
  readonly isTauri: boolean;

  constructor() {
    this.isTauri = typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__?.invoke);
  }

  async loadSnapshot(fixture: FixturePreset): Promise<KestryxIpcSnapshot> {
    if (this.isTauri && window.__TAURI_INTERNALS__) {
      try {
        return await window.__TAURI_INTERNALS__.invoke<KestryxIpcSnapshot>('kestryx_load_snapshot', {
          fixture,
        });
      } catch {
        // Fallback to deterministic fixture state if command is not registered
      }
    }

    return {
      runtimeMode: this.isTauri ? 'tauri-native' : 'mock-browser',
      fixture,
      instances: buildInstancesForPreset(fixture),
      mods: buildModsForPreset(fixture),
      worlds: structuredClone(TYPICAL_WORLDS),
      servers: structuredClone(TYPICAL_SERVERS),
      accounts: buildDefaultAccounts(),
      javaRuntimes: structuredClone(TYPICAL_JAVA_RUNTIMES),
      crashes: structuredClone(TYPICAL_CRASH_SIGNATURES),
      discoverCatalog: structuredClone(DISCOVER_CATALOG),
    };
  }

  computeOfflineUuid(username: string) {
    return computeOfflinePlayerUuid(username);
  }

   redactSensitiveLog(rawText: string): string {
    return rawText
      .replace(/eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/g, '[REDACTED_JWT_TOKEN]')
      .replace(/(--accessToken\s+)(\S+)/gi, '$1[REDACTED_ACCESS_TOKEN]')
      .replace(/(C:\\Users\\|\/Users\/|\/home\/)([^\\/]+)(\\?\/?)/g, '$1<USER>$3')
      .replace(/(XBL3\.0 x=[^\s;]+)/g, '[REDACTED_XSTS_HEADER]');
  }
}

export const ipcAdapter = new DualModeIpcAdapter();
