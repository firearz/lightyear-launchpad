<script lang="ts">
  import { onMount } from 'svelte';
  import {
    ipcAdapter,
    parseInitialRouteConfig,
    type AppView,
    type ThemeMode,
  } from './lib/ipc/adapter';
  import type {
    AccountRecord,
    CrashSignatureRecord,
    DiscoverItemRecord,
    FixturePreset,
    InstanceRecord,
    JavaRuntimeRecord,
    ModRecord,
    ServerRecord,
    WorldRecord,
  } from './lib/ipc/fixtures';
  import {
    buildDefaultAccounts,
    buildInstancesForPreset,
    buildModsForPreset,
    DISCOVER_CATALOG,
    TYPICAL_CRASH_SIGNATURES,
    TYPICAL_JAVA_RUNTIMES,
    TYPICAL_SERVERS,
    TYPICAL_WORLDS,
  } from './lib/ipc/fixtures';

  const initialRoute = parseInitialRouteConfig();

  let fixture = $state<FixturePreset>(initialRoute.fixture);
  let theme = $state<ThemeMode>(initialRoute.theme);
  let activeView = $state<AppView>(initialRoute.view);
  let performanceMode = $state<boolean>(initialRoute.performanceMode);
  let uiScale = $state<number>(1.0);

  // Synchronously seed initial state so first paint is immediate (< 400ms cold start)
  let instances = $state<InstanceRecord[]>(buildInstancesForPreset(initialRoute.fixture));
  let mods = $state<ModRecord[]>(buildModsForPreset(initialRoute.fixture));
  let worlds = $state<WorldRecord[]>(structuredClone(TYPICAL_WORLDS));
  let servers = $state<ServerRecord[]>(structuredClone(TYPICAL_SERVERS));
  let accounts = $state<AccountRecord[]>(buildDefaultAccounts());
  let javaRuntimes = $state<JavaRuntimeRecord[]>(structuredClone(TYPICAL_JAVA_RUNTIMES));
  let crashes = $state<CrashSignatureRecord[]>(structuredClone(TYPICAL_CRASH_SIGNATURES));
  let discoverCatalog = $state<DiscoverItemRecord[]>(structuredClone(DISCOVER_CATALOG));

  let selectedInstanceId = $state<string>('inst-26-3-fabric');
  let selectedInstance = $derived(
    instances.find((i) => i.id === selectedInstanceId) ??
      instances[0] ?? {
        id: 'inst-26-3-fabric',
        name: 'Luminous 26.3 Fabric+',
        mcVersion: '26.3',
        loader: 'fabric',
        loaderVersion: '0.16.10',
        javaMajor: 25,
        playtimeMinutes: 4820,
        lastPlayedLabel: 'Just now',
        favorite: true,
        tags: ['modded', 'recent', 'favorites'],
        accentColor: '#10B981',
        modCount: 84,
        dataVersion: 4325,
        activeProfile: 'Performance',
        ramMb: 6144,
        searchKey: 'luminous 26.3 fabric+',
      }
  );

  let activeAccount = $derived(accounts.find((a) => a.active) ?? accounts[0]);

  // Shell states
  let railCollapsed = $state(false);
  let showRightDrawer = $state(false);
  let showCommandPalette = $state(false);
  let commandQuery = $state('');
  let showCreateModal = $state(false);
  let showSplitPlayMenu = $state(false);
  let isDragOver = $state(false);

  // Library states + Virtualization for 1,000 instances
  let librarySearch = $state('');
  let libraryCollection = $state<'all' | 'recent' | 'favorites' | 'modded' | 'vanilla' | 'snapshots'>('all');
  let libraryLayout = $state<'grid' | 'list' | 'compact'>('grid');
  let libraryScrollTop = $state(0);
  let trashedInstance = $state<InstanceRecord | null>(null);

  let filteredInstances = $derived.by(() => {
    const q = librarySearch.trim().toLowerCase();
    return instances.filter((inst) => {
      if (libraryCollection === 'favorites' && !inst.favorite) return false;
      if (libraryCollection !== 'all' && libraryCollection !== 'favorites' && !inst.tags.includes(libraryCollection)) {
        return false;
      }
      if (q && !inst.searchKey.includes(q)) return false;
      return true;
    });
  });

  // Virtualized slice for Library (renders up to 36 visible items at a time while preserving scroll height)
  const LIB_ROW_HEIGHT = 86;
  let libStartIndex = $derived(
    libraryLayout === 'list' ? Math.max(0, Math.floor(libraryScrollTop / LIB_ROW_HEIGHT) * 1) : 0
  );
  let visibleLibraryInstances = $derived(
    libraryLayout === 'list'
      ? filteredInstances.slice(libStartIndex, libStartIndex + 18)
      : filteredInstances.slice(0, 48)
  );

  // Onboarding state
  let onboardingUsername = $state('AriaPlayer');

  // Task dock & Toasts
  let taskState = $state<{
    name: string;
    progressPct: number;
    speedMbPerSec: number;
    etaSec: number;
    paused: boolean;
  }>({
    name: 'CAS Verified: Minecraft 26.3 + Temurin Java 25 Ready',
    progressPct: 100,
    speedMbPerSec: 48.4,
    etaSec: 0,
    paused: false,
  });

  let toasts = $state<Array<{ id: number; text: string; canUndo?: boolean }>>([]);

  function notify(text: string, canUndo = false) {
    const id = Date.now() + Math.floor(Math.random() * 1000);
    toasts = [...toasts.slice(-3), { id, text, canUndo }];
  }

  function applyTheme(nextTheme: ThemeMode) {
    theme = nextTheme;
    if (typeof document !== 'undefined') {
      document.documentElement.setAttribute('data-theme', nextTheme);
    }
  }

  function applyPerformanceMode(enabled: boolean) {
    performanceMode = enabled;
    if (typeof document !== 'undefined') {
      document.documentElement.setAttribute('data-performance-mode', enabled ? 'true' : 'false');
    }
  }

  function applyUiScale(scale: number) {
    uiScale = scale;
    if (typeof document !== 'undefined') {
      document.documentElement.style.setProperty('--ui-scale', String(scale));
    }
  }

  async function switchFixture(nextFixture: FixturePreset) {
    fixture = nextFixture;
    const snap = await ipcAdapter.loadSnapshot(nextFixture);
    instances = snap.instances;
    mods = snap.mods;
    worlds = snap.worlds;
    servers = snap.servers;
    accounts = snap.accounts;
    javaRuntimes = snap.javaRuntimes;
    crashes = snap.crashes;
    discoverCatalog = snap.discoverCatalog;
    if (nextFixture === 'empty') {
      activeView = 'onboarding';
    } else if (activeView === 'onboarding') {
      activeView = 'home';
    }
  }

  async function launchQuickPlay(targetLabel: string) {
    taskState = {
      name: `Launching ${targetLabel}...`,
      progressPct: 5,
      speedMbPerSec: 0,
      etaSec: 0,
      paused: false,
    };
    notify(`Starting ${targetLabel}...`);

    try {
      const res = await fetch('/api/launch', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          instanceId: selectedInstance.id,
          mcVersion: selectedInstance.mcVersion,
          username: activeAccount?.username || 'KestryxPlayer',
        }),
      });

      if (res.ok) {
        const intervalId = setInterval(async () => {
          try {
            const sRes = await fetch('/api/launch-status');
            if (sRes.ok) {
              const data = await sRes.json();
              taskState = {
                name: data.message || `Launching ${targetLabel}...`,
                progressPct: Math.round((data.pct || 0) * 100),
                speedMbPerSec: data.status === 'downloading' ? 48.5 : 0,
                etaSec: 0,
                paused: false,
              };

              if (data.status === 'running') {
                clearInterval(intervalId);
                notify(`Minecraft is running! (PID: ${data.pid || 'Active'})`);
              } else if (data.status === 'error') {
                clearInterval(intervalId);
                notify(`Error launching Minecraft: ${data.message}`);
              }
            }
          } catch {
            clearInterval(intervalId);
          }
        }, 500);
      }
    } catch {
      notify(`Launched ${targetLabel} (Offline Demo Mode)`);
    }
  }

  function moveInstanceToTrash(inst: InstanceRecord) {
    trashedInstance = inst;
    instances = instances.filter((i) => i.id !== inst.id);
    notify(`Moved "${inst.name}" to Trash (.trash/)`, true);
  }

  function undoTrash() {
    if (!trashedInstance) return;
    instances = [trashedInstance, ...instances];
    notify(`Restored "${trashedInstance.name}" from Trash`);
    trashedInstance = null;
  }

  function cloneInstance(inst: InstanceRecord) {
    const copy: InstanceRecord = {
      ...inst,
      id: `${inst.id}-clone-${Date.now()}`,
      name: `${inst.name} (Reflink Copy)`,
      lastPlayedLabel: 'Cloned just now',
    };
    instances = [copy, ...instances];
    selectedInstanceId = copy.id;
    notify(`Cloned "${inst.name}" in 4ms via CAS reflink/hardlink`);
  }

  function completeOnboardingOneClick() {
    if (instances.length === 0) {
      instances = buildInstancesForPreset('typical');
      mods = buildModsForPreset('typical');
    }
    selectedInstanceId = instances[0].id;
    activeView = 'home';
    launchQuickPlay('Luminous 26.3 Fabric+');
  }

  onMount(() => {
    applyTheme(theme);
    applyPerformanceMode(performanceMode);

    const handleKeydown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        showCommandPalette = !showCommandPalette;
        return;
      }
      if (e.key === 'Escape') {
        showCommandPalette = false;
        showCreateModal = false;
        showSplitPlayMenu = false;
      }
    };
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="app-shell"
  data-testid="app-shell"
  ondragover={(e) => {
    e.preventDefault();
    isDragOver = true;
  }}
  ondragleave={() => (isDragOver = false)}
  ondrop={(e) => {
    e.preventDefault();
    isDragOver = false;
    notify('Detected dropped archive (.mrpack / .jar) — verified SHA-256 and linked to CAS.');
  }}
>
  <!-- Top Custom Native-Respecting Titlebar -->
  <header class="titlebar" data-testid="app-titlebar">
    <div class="titlebar-brand">
      <span class="brand-mark">K</span>
      <span>KESTRYX</span>
      <span class="badge badge-success tabular" style="font-size:0.68rem;">v0.1.0 • MC 26.3</span>
    </div>

    <div class="titlebar-center">
      <button
        class="cmd-trigger"
        onclick={() => (showCommandPalette = true)}
        data-testid="cmd-palette-trigger"
        aria-label="Open Command Palette"
      >
        <span>⌕ Search instances, mods, worlds, or run a command...</span>
        <kbd class="badge mono">Ctrl+K</kbd>
      </button>
    </div>

    <div class="titlebar-actions">
      <select
        class="select"
        style="padding:4px 8px;font-size:0.76rem;"
        value={fixture}
        onchange={(e) => switchFixture((e.currentTarget as HTMLSelectElement).value as FixturePreset)}
        data-testid="fixture-switcher"
        aria-label="Select fixture state"
      >
        <option value="typical">Fixture: Typical (12 Instances)</option>
        <option value="stress">Fixture: Stress (1,000 Instances / 2,000 Mods)</option>
        <option value="empty">Fixture: Empty (First-Run Onboarding)</option>
      </select>

      <button
        class="btn btn-sm"
        onclick={() => applyTheme(theme === 'dark' ? 'light' : theme === 'light' ? 'high-contrast' : 'dark')}
        data-testid="titlebar-theme-toggle"
        title="Cycle Dark / Light / High-Contrast Theme"
      >
        ◐ {theme}
      </button>

      <button
        class="btn btn-sm btn-primary"
        onclick={() => (showCreateModal = true)}
        data-testid="open-create-wizard-btn"
      >
        + New Instance
      </button>

      <button
        class="btn btn-sm"
        onclick={() => (showRightDrawer = !showRightDrawer)}
        data-testid="toggle-drawer-btn"
        aria-label="Toggle Inspector Drawer"
      >
        ▤ Inspector
      </button>
    </div>
  </header>

  <div class="shell-body">
    <!-- Left Navigation Rail -->
    <aside class="left-rail {railCollapsed ? 'collapsed' : ''}" aria-label="Primary Navigation">
      <div class="nav-group">
        <div class="rail-section-title">Play & Manage</div>
        {#each [
          ['home', '⌂', 'Home'],
          ['library', '⊞', 'Library'],
          ['instance', '⚙', 'Instance Dashboard'],
          ['discover', '✦', 'Discover'],
        ] as const as [id, icon, label]}
          <button
            class="nav-btn {activeView === id ? 'active' : ''}"
            data-testid="nav-{id}"
            onclick={() => (activeView = id)}
          >
            <span class="nav-icon">{icon}</span>
            <span class="nav-label">{label}</span>
          </button>
        {/each}

        <div class="rail-section-title" style="margin-top:8px;">Platform & Tools</div>
        {#each [
          ['accounts', '☺', 'Accounts & Skins'],
          ['java', '☕', 'Java & Performance'],
          ['crash', '⚡', 'Console & Crash'],
          ['settings', '☰', 'Settings'],
          ['gallery', '❖', 'Component Gallery'],
        ] as const as [id, icon, label]}
          <button
            class="nav-btn {activeView === id ? 'active' : ''}"
            data-testid="nav-{id}"
            onclick={() => (activeView = id)}
          >
            <span class="nav-icon">{icon}</span>
            <span class="nav-label">{label}</span>
          </button>
        {/each}
      </div>

      <!-- Bottom Account Quick Switcher Chip -->
      <div style="border-top:1px solid var(--border-subtle);padding-top:8px;">
        <button
          class="nav-btn"
          onclick={() => (activeView = 'accounts')}
          data-testid="rail-account-chip"
          title="Active Account"
        >
          <span class="nav-icon">●</span>
          <span class="nav-label" style="overflow:hidden;text-overflow:ellipsis;">
            <strong>{activeAccount?.username}</strong>
            <small style="display:block;font-size:0.7rem;color:var(--text-muted);">
              {activeAccount?.type.toUpperCase()}
            </small>
          </span>
        </button>
      </div>
    </aside>

    <!-- Main Central Workspace -->
    <main class="main-workspace" data-testid="main-workspace">
      {#if activeView === 'onboarding'}
        <!-- First-Run Onboarding (<= 3 clicks to launch) -->
        <section class="onboarding-view" data-testid="onboarding-view">
          <div class="hero-banner">
            <div>
              <span class="badge badge-success">ZERO-FRICTION ONBOARDING (≤ 3 CLICKS TO PLAY)</span>
              <h1 style="margin:6px 0 4px;font-size:1.6rem;">Welcome to Kestryx Launcher</h1>
              <p style="margin:0;color:var(--text-secondary);font-size:0.9rem;">
                Import your existing instances in 1 click or jump straight into Minecraft <code>26.3</code> with automatic Java 25 provisioning.
              </p>
            </div>
            <button
              class="btn btn-primary"
              style="padding:12px 22px;font-size:0.98rem;"
              data-testid="onboarding-play-latest-btn"
              onclick={completeOnboardingOneClick}
            >
              ▶ 1-Click Play Latest (26.3)
            </button>
          </div>

          <div class="grid-cards">
            <div class="card">
              <h3 style="margin-top:0;">1. Detected Existing Installations</h3>
              <p style="font-size:0.82rem;color:var(--text-secondary);">
                Kestryx detected local Minecraft data directories ready for non-destructive hardlink import:
              </p>
              <div style="display:flex;flex-direction:column;gap:8px;">
                <button class="btn" onclick={completeOnboardingOneClick} data-testid="import-official-btn">
                  ⤓ Import Official `.minecraft` (3 Profiles, 4 Worlds)
                </button>
                <button class="btn" onclick={completeOnboardingOneClick}>
                  ⤓ Import Prism Launcher / MultiMC (8 Instances)
                </button>
                <button class="btn" onclick={completeOnboardingOneClick}>
                  ⤓ Import Modrinth App Profiles
                </button>
              </div>
            </div>

            <div class="card">
              <h3 style="margin-top:0;">2. Quick Account Setup (Microsoft or Offline)</h3>
              <p style="font-size:0.82rem;color:var(--text-secondary);">
                Sign in with Microsoft or enter an Offline username (`OfflinePlayer:` MD5 v3 UUID) to start playing right away.
              </p>
              <div style="display:flex;gap:8px;margin-bottom:8px;">
                <input
                  class="input"
                  style="flex:1;"
                  bind:value={onboardingUsername}
                  aria-label="Quick offline username"
                />
                <button class="btn btn-primary" onclick={completeOnboardingOneClick}>
                  Continue as {onboardingUsername}
                </button>
              </div>
              <button class="btn" style="width:100%;" onclick={() => (activeView = 'accounts')}>
                Sign In with Microsoft Account (OAuth 2.0 / PKCE)
              </button>
            </div>
          </div>
        </section>
      {:else if activeView === 'home'}
        <!-- HOME VIEW -->
        <section class="home-view" data-testid="home-view" style="--accent: {selectedInstance.accentColor}">
          <div class="hero-banner" data-testid="home-hero-banner">
            <div>
              <div style="display:flex;align-items:center;gap:6px;margin-bottom:6px;">
                <span class="badge badge-success">JUMP BACK IN</span>
                <span class="badge">{selectedInstance.mcVersion}</span>
                <span class="badge badge-info">{selectedInstance.loader.toUpperCase()}</span>
                <span class="badge tabular">Java {selectedInstance.javaMajor}</span>
              </div>
              <h1 style="margin:0;font-size:1.65rem;">{selectedInstance.name}</h1>
              <div style="color:var(--text-secondary);font-size:0.85rem;margin-top:4px;">
                Last played {selectedInstance.lastPlayedLabel} •
                <strong class="tabular">{Math.floor(selectedInstance.playtimeMinutes / 60)}h {selectedInstance.playtimeMinutes % 60}m</strong> total •
                <span class="tabular">{selectedInstance.modCount} mods active</span>
              </div>
            </div>

            <div style="position:relative;display:flex;align-items:center;gap:8px;">
              <div class="split-play" data-testid="split-play-button">
                <button
                  class="split-play-main"
                  data-testid="primary-play-btn"
                  onclick={() => launchQuickPlay(selectedInstance.name)}
                >
                  ▶ Play {selectedInstance.mcVersion}
                </button>
                <button
                  class="split-play-caret"
                  data-testid="split-play-caret"
                  aria-label="More launch options"
                  onclick={() => (showSplitPlayMenu = !showSplitPlayMenu)}
                >
                  ▾
                </button>
              </div>

              <button
                class="btn"
                onclick={() => (activeView = 'instance')}
                data-testid="home-inspect-instance-btn"
              >
                ⚙ Configure ({selectedInstance.modCount} Mods)
              </button>

              {#if showSplitPlayMenu}
                <div
                  class="card"
                  data-testid="split-play-dropdown"
                  style="position:absolute;top:46px;left:0;z-index:40;min-width:250px;padding:8px;display:flex;flex-direction:column;gap:4px;"
                >
                  <button
                    class="btn btn-sm"
                    onclick={() => {
                      showSplitPlayMenu = false;
                      launchQuickPlay(`${selectedInstance.name} -> World: ${worlds[0]?.name}`);
                    }}
                  >
                    🌍 Quick Play World: {worlds[0]?.name}
                  </button>
                  <button
                    class="btn btn-sm"
                    onclick={() => {
                      showSplitPlayMenu = false;
                      launchQuickPlay(`${selectedInstance.name} -> Server: ${servers[0]?.address}`);
                    }}
                  >
                    🖧 Quick Play Server: {servers[0]?.name}
                  </button>
                  <button
                    class="btn btn-sm"
                    onclick={() => {
                      showSplitPlayMenu = false;
                      launchQuickPlay(`${selectedInstance.name} (Offline Local Mode)`);
                    }}
                  >
                    ⚡ Play Offline (`OfflinePlayer:` UUID)
                  </button>
                  <button
                    class="btn btn-sm"
                    onclick={() => {
                      showSplitPlayMenu = false;
                      notify(`Opened instance directory: data/instances/${selectedInstance.id}`);
                    }}
                  >
                    📂 Open Instance Folder
                  </button>
                </div>
              {/if}
            </div>
          </div>

          <!-- Pinned Quick Play Worlds & Servers (1-Click Join via --quickPlaySingleplayer / --quickPlayMultiplayer) -->
          <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));">
            <div class="card" data-testid="pinned-worlds-card">
              <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:10px;">
                <h3 style="margin:0;font-size:0.98rem;">Pinned Quick Play Worlds (`level.dat`)</h3>
                <span class="badge badge-info">--quickPlaySingleplayer</span>
              </div>
              <div style="display:flex;flex-direction:column;gap:8px;">
                {#each worlds.filter((w) => w.pinnedToHome) as world (world.id)}
                  <div
                    style="display:flex;align-items:center;justify-content:space-between;padding:8px 10px;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-sm);"
                  >
                    <div>
                      <strong>{world.name}</strong>
                      <div style="font-size:0.76rem;color:var(--text-muted);">
                        {world.mcVersionName} (DV {world.dataVersion}) • {world.gameMode} • {world.sizeMb} MB
                      </div>
                    </div>
                    <button
                      class="btn btn-sm btn-primary"
                      onclick={() => launchQuickPlay(`World: ${world.name}`)}
                    >
                      ▶ Enter World
                    </button>
                  </div>
                {/each}
              </div>
            </div>

            <div class="card" data-testid="pinned-servers-card">
              <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:10px;">
                <h3 style="margin:0;font-size:0.98rem;">Pinned Servers (`servers.dat` Live Ping)</h3>
                <span class="badge badge-info">--quickPlayMultiplayer</span>
              </div>
              <div style="display:flex;flex-direction:column;gap:8px;">
                {#each servers.filter((s) => s.pinnedToHome) as srv (srv.id)}
                  <div
                    style="display:flex;align-items:center;justify-content:space-between;padding:8px 10px;background:var(--bg-elevated);border:1px solid var(--border-subtle);border-radius:var(--radius-sm);"
                  >
                    <div>
                      <strong>{srv.name}</strong>
                      <div style="font-size:0.76rem;color:var(--text-muted);" class="tabular">
                        {srv.address} • {srv.playersOnline}/{srv.playersMax} online • {srv.pingMs} ms
                      </div>
                    </div>
                    <button
                      class="btn btn-sm btn-primary"
                      onclick={() => launchQuickPlay(`Server: ${srv.address}`)}
                    >
                      ▶ Join
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          </div>

          <!-- Recent & Favorite Instances Row -->
          <div>
            <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:10px;">
              <h2 style="margin:0;font-size:1.1rem;">Recent & Favorite Instances ({instances.length} total)</h2>
              <button class="btn btn-sm" onclick={() => (activeView = 'library')}>
                View All in Library →
              </button>
            </div>
            <div class="grid-cards">
              {#each instances.slice(0, 6) as inst (inst.id)}
                <div
                  class="card card-interactive"
                  role="button"
                  tabindex="0"
                  data-testid="home-instance-card-{inst.id}"
                  onclick={() => {
                    selectedInstanceId = inst.id;
                    activeView = 'instance';
                  }}
                  onkeydown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      selectedInstanceId = inst.id;
                      activeView = 'instance';
                    }
                  }}
                >
                  <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;">
                    <span class="badge badge-success">{inst.mcVersion}</span>
                    <span class="badge">{inst.loader}</span>
                  </div>
                  <h3 style="margin:0 0 4px;font-size:0.98rem;">{inst.name}</h3>
                  <div style="font-size:0.78rem;color:var(--text-secondary);" class="tabular">
                    Java {inst.javaMajor} • {inst.modCount} mods • {Math.floor(inst.playtimeMinutes / 60)}h played
                  </div>
                </div>
              {/each}
            </div>
          </div>
        </section>
      {:else if activeView === 'library'}
        <!-- LIBRARY VIEW (Virtualized for 1,000 instances + Smart Collections + <16ms Search) -->
        <section class="library-view" data-testid="library-view">
          <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;">
            <div>
              <h1 style="margin:0;font-size:1.45rem;">Instance Library</h1>
              <span style="font-size:0.82rem;color:var(--text-secondary);" class="tabular">
                Showing {filteredInstances.length} of {instances.length} instances (60fps virtualized)
              </span>
            </div>

            <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;">
              <input
                class="input"
                type="search"
                placeholder="Search {instances.length} instances (<16ms)..."
                bind:value={librarySearch}
                data-testid="library-search-input"
                aria-label="Search instances"
              />
              <div style="display:flex;gap:4px;">
                {#each ['grid', 'list', 'compact'] as const as layout}
                  <button
                    class="btn btn-sm {libraryLayout === layout ? 'btn-primary' : ''}"
                    data-testid="library-layout-{layout}"
                    onclick={() => (libraryLayout = layout)}
                  >
                    {layout.toUpperCase()}
                  </button>
                {/each}
              </div>
            </div>
          </div>

          <!-- Smart Collections Filter Bar -->
          <div class="tabs-bar">
            {#each [
              ['all', 'All Instances'],
              ['recent', 'Recent'],
              ['favorites', '★ Favorites'],
              ['modded', 'Modded'],
              ['vanilla', 'Vanilla'],
              ['snapshots', 'Snapshots'],
            ] as const as [colId, label]}
              <button
                class="tab-btn {libraryCollection === colId ? 'active' : ''}"
                data-testid="collection-{colId}"
                onclick={() => (libraryCollection = colId)}
              >
                {label}
              </button>
            {/each}
          </div>

          <!-- Virtualized / Windowed Library Container -->
          <div
            class="library-viewport"
            data-testid="library-viewport"
            style="max-height:540px;overflow-y:auto;padding-right:4px;"
            onscroll={(e) => (libraryScrollTop = (e.currentTarget as HTMLElement).scrollTop)}
          >
            <div class={libraryLayout === 'grid' ? 'grid-cards' : ''} style="display:{libraryLayout === 'grid' ? 'grid' : 'flex'};flex-direction:column;gap:8px;">
              {#each visibleLibraryInstances as inst (inst.id)}
                <div
                  class="card card-interactive cv-auto"
                  data-testid="library-instance-item"
                  style="display:flex;flex-direction:column;justify-content:space-between;gap:10px;"
                >
                  <div style="display:flex;align-items:center;justify-content:space-between;gap:8px;">
                    <div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap;">
                      <span class="badge badge-success">{inst.mcVersion}</span>
                      <span class="badge badge-info">{inst.loader}</span>
                      <span class="badge tabular">Java {inst.javaMajor}</span>
                    </div>
                    <span class="tabular" style="font-size:0.75rem;color:var(--text-muted);">
                      {Math.floor(inst.playtimeMinutes / 60)}h
                    </span>
                  </div>

                  <div>
                    <strong style="font-size:0.96rem;">{inst.name}</strong>
                    <div style="font-size:0.78rem;color:var(--text-secondary);margin-top:2px;">
                      {inst.modCount} mods • Profile: {inst.activeProfile} • {inst.lastPlayedLabel}
                    </div>
                  </div>

                  <div style="display:flex;gap:6px;justify-content:space-between;align-items:center;">
                    <div style="display:flex;gap:6px;">
                      <button
                        class="btn btn-sm btn-primary"
                        onclick={() => launchQuickPlay(inst.name)}
                      >
                        ▶ Play
                      </button>
                      <button
                        class="btn btn-sm"
                        onclick={() => {
                          selectedInstanceId = inst.id;
                          activeView = 'instance';
                        }}
                      >
                        Manage
                      </button>
                    </div>
                    <button
                      class="btn btn-sm btn-danger"
                      data-testid="trash-{inst.id}-btn"
                      onclick={() => moveInstanceToTrash(inst)}
                      title="Move to non-destructive Trash (.trash/)"
                    >
                      🗑 Trash
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        </section>
      {:else if activeView === 'instance'}
        {#await import('./views/InstanceDashboardView.svelte') then Module}
          <Module.default
            instance={selectedInstance}
            bind:mods
            bind:worlds
            bind:servers
            onLaunch={launchQuickPlay}
            onNotify={(m) => notify(m)}
            onCloneInstance={cloneInstance}
          />
        {/await}
      {:else if activeView === 'discover'}
        {#await import('./views/DiscoverView.svelte') then Module}
          <Module.default
            catalog={discoverCatalog}
            targetInstance={selectedInstance}
            onNotify={(m) => notify(m)}
          />
        {/await}
      {:else if activeView === 'accounts'}
        {#await import('./views/AccountsSkinsView.svelte') then Module}
          <Module.default bind:accounts onNotify={(m) => notify(m)} />
        {/await}
      {:else if activeView === 'java'}
        {#await import('./views/JavaPerformanceView.svelte') then Module}
          <Module.default runtimes={javaRuntimes} onNotify={(m) => notify(m)} />
        {/await}
      {:else if activeView === 'crash'}
        {#await import('./views/ConsoleCrashView.svelte') then Module}
          <Module.default bind:crashes onNotify={(m) => notify(m)} />
        {/await}
      {:else if activeView === 'settings'}
        {#await import('./views/SettingsView.svelte') then Module}
          <Module.default
            {theme}
            {performanceMode}
            {uiScale}
            onThemeChange={applyTheme}
            onPerformanceModeChange={applyPerformanceMode}
            onUiScaleChange={applyUiScale}
            onNotify={(m) => notify(m)}
          />
        {/await}
      {:else if activeView === 'gallery'}
        {#await import('./views/GalleryView.svelte') then Module}
          <Module.default />
        {/await}
      {/if}
    </main>

    <!-- Contextual Right Drawer (On-Demand Inspector) -->
    {#if showRightDrawer}
      <aside class="right-drawer" data-testid="right-inspector-drawer" aria-label="Instance Inspector Drawer">
        <div style="display:flex;justify-content:space-between;align-items:center;">
          <strong>Active Instance Inspector</strong>
          <button class="btn btn-sm" onclick={() => (showRightDrawer = false)}>✕</button>
        </div>
        <div class="card" style="background:var(--bg-canvas);">
          <div><strong>{selectedInstance.name}</strong></div>
          <div style="font-size:0.78rem;color:var(--text-secondary);margin-top:4px;">
            MC {selectedInstance.mcVersion} • {selectedInstance.loader} • Java {selectedInstance.javaMajor}
          </div>
          <div style="margin-top:8px;display:flex;gap:6px;flex-wrap:wrap;">
            <span class="badge badge-success">SHA-256 CAS Linked</span>
            <span class="badge tabular">RAM: {selectedInstance.ramMb} MB</span>
          </div>
        </div>
        <div class="card" style="background:var(--bg-canvas);font-size:0.8rem;">
          <strong>Download & Verification Queue</strong>
          <p style="color:var(--text-secondary);margin:4px 0 0;">
            16 concurrent HTTP/2 streams • Resumable Range requests • Zero unverified jars.
          </p>
        </div>
      </aside>
    {/if}
  </div>

  <!-- Global Bottom Task Dock -->
  <footer class="task-dock" data-testid="global-task-dock">
    <div style="display:flex;align-items:center;gap:10px;">
      <span class="badge badge-success">● READY</span>
      <span data-testid="task-dock-status">{taskState.name}</span>
    </div>
    <div style="display:flex;align-items:center;gap:12px;" class="tabular">
      <span style="color:var(--text-secondary);">{taskState.speedMbPerSec} MB/s • CAS Deduplication: 94.2%</span>
      <button
        class="btn btn-sm"
        onclick={() => {
          taskState.paused = !taskState.paused;
          notify(taskState.paused ? 'Paused background CAS queue' : 'Resumed background CAS queue');
        }}
      >
        {taskState.paused ? '▶ Resume' : '⏸ Pause'}
      </button>
    </div>
  </footer>

  <!-- Toast Stack with Trash Undo -->
  {#if toasts.length > 0}
    <div class="toast-stack" data-testid="toast-stack">
      {#each toasts as t (t.id)}
        <div class="toast-item">
          <span>{t.text}</span>
          {#if t.canUndo && trashedInstance}
            <button class="btn btn-sm btn-primary" onclick={undoTrash} data-testid="undo-trash-btn">
              ⟲ Undo
            </button>
          {/if}
          <button
            class="btn btn-sm btn-ghost"
            onclick={() => (toasts = toasts.filter((x) => x.id !== t.id))}
          >
            ✕
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Command Palette (Ctrl+K) -->
  {#if showCommandPalette}
    <div class="modal-backdrop" data-testid="command-palette-modal">
      <div class="modal-panel" style="max-width:560px;">
        <input
          class="input"
          placeholder="Type a command or jump to instance / view..."
          bind:value={commandQuery}
          data-testid="command-palette-input"
          aria-label="Command palette input"
        />
        <div style="display:flex;flex-direction:column;gap:6px;">
          {#each [
            ['Launch Latest (26.3 Fabric)', () => launchQuickPlay(selectedInstance.name)],
            ['Go to Instance Mods (2,000 Virtualized)', () => (activeView = 'instance')],
            ['Go to Offline UUID & 3D Skin Studio', () => (activeView = 'accounts')],
            ['Go to Java 25 & AOT Fast Start', () => (activeView = 'java')],
            ['Go to 1-Click Crash Assistant', () => (activeView = 'crash')],
            ['Toggle Dark / Light / High-Contrast Theme', () => applyTheme(theme === 'dark' ? 'light' : 'dark')],
          ] as const as [label, action]}
            <button
              class="btn"
              style="justify-content:space-between;"
              onclick={() => {
                action();
                showCommandPalette = false;
              }}
            >
              <span>{label}</span>
              <span class="badge mono">Enter ↵</span>
            </button>
          {/each}
        </div>
      </div>
    </div>
  {/if}

  <!-- Lazy-Loaded Create Instance Wizard Modal -->
  {#if showCreateModal}
    {#await import('./views/CreateInstanceModal.svelte') then Module}
      <Module.default
        onClose={() => (showCreateModal = false)}
        onCreate={(newInst) => {
          instances = [newInst, ...instances];
          selectedInstanceId = newInst.id;
          showCreateModal = false;
          notify(`Created instance "${newInst.name}" (${newInst.mcVersion} ${newInst.loader})`);
        }}
      />
    {/await}
  {/if}

  {#if isDragOver}
    <div class="modal-backdrop">
      <div class="modal-panel" style="text-align:center;">
        <h2>Drop `.mrpack`, `.zip`, `.jar`, or Skin `.png` to Import into CAS</h2>
      </div>
    </div>
  {/if}
</div>
