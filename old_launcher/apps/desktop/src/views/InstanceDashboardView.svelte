<script lang="ts">
  import type { InstanceRecord, ModRecord, ServerRecord, WorldRecord } from '../lib/ipc/fixtures';
  import { ipcAdapter } from '../lib/ipc/adapter';

  interface Props {
    instance: InstanceRecord;
    mods: ModRecord[];
    worlds: WorldRecord[];
    servers: ServerRecord[];
    onLaunch: (target: string) => void;
    onNotify: (msg: string) => void;
    onCloneInstance: (inst: InstanceRecord) => void;
  }

  let {
    instance,
    mods = $bindable(),
    worlds = $bindable(),
    servers = $bindable(),
    onLaunch,
    onNotify,
    onCloneInstance,
  }: Props = $props();

  const TABS = [
    'Overview',
    'Mods',
    'Resource Packs',
    'Shaders',
    'Worlds',
    'Servers',
    'Screenshots',
    'Logs',
    'Backups',
    'Files',
    'Settings',
  ] as const;

  type TabName = (typeof TABS)[number];
  let activeTab = $state<TabName>('Mods');

  // Overview dry-run switcher state
  let targetVersion = $state('26.3');
  let targetLoader = $state<'fabric' | 'neoforge' | 'forge' | 'quilt' | 'vanilla'>('fabric');
  let dryRunReport = $state<string | null>(null);

  // Mods tab state + virtualized scroll
  let modSearch = $state('');
  let modScrollTop = $state(0);
  let activeModProfile = $state<'Default' | 'Performance' | 'Full'>('Performance');
  let showProfileDiff = $state(false);
  let historySnapshotCount = $state(3);
  const ROW_HEIGHT = 44;
  const VIEWPORT_ROWS = 14;

  let filteredMods = $derived.by(() => {
    const q = modSearch.trim().toLowerCase();
    if (!q) return mods;
    return mods.filter((m) => m.searchKey.includes(q));
  });

  let startIdx = $derived(Math.max(0, Math.floor(modScrollTop / ROW_HEIGHT)));
  let visibleMods = $derived(filteredMods.slice(startIdx, startIdx + VIEWPORT_ROWS));
  let totalModsHeight = $derived(filteredMods.length * ROW_HEIGHT);

  // World Downgrade Guard state
  let downgradeWarningWorld = $state<WorldRecord | null>(null);
  let simulatedTargetDowngradeVersion = $state('1.21.4 (DataVersion 4189)');

  // Logs tab state
  let logFilter = $state('');
  let logLevel = $state<'ALL' | 'INFO' | 'WARN' | 'ERROR'>('ALL');
  let rawLogs = $state([
    '[19:42:01] [main/INFO]: Loading Minecraft 26.3 with Fabric Loader 0.16.10 (Java 25.0.1)',
    '[19:42:01] [main/INFO]: Launching with arguments: --username AriaValkyrie --accessToken eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secretPayload.sigValue --gameDir C:\\Users\\AriaValkyrie\\AppData\\Roaming\\Kestryx',
    '[19:42:02] [Render thread/INFO]: [Sodium] OpenGL 4.6 NVIDIA RTX 4080 (driver 572.16) initialized',
    '[19:42:03] [Render thread/WARN]: [Iris] Shader pack ComplementaryReimagined_r5.4 uses fallback shadow map resolution',
    '[19:42:05] [Worker-Main-4/INFO]: Loaded 84 mods via CAS hardlink pool in 142 ms',
    '[19:42:08] [Render thread/ERROR]: [Mixin] Optional compat target net.optifine.Config not present (ignored safely)',
  ]);

  let filteredLogs = $derived.by(() => {
    return rawLogs.filter((line) => {
      if (logLevel !== 'ALL' && !line.includes(`/${logLevel}]`)) return false;
      if (logFilter.trim() && !line.toLowerCase().includes(logFilter.trim().toLowerCase())) return false;
      return true;
    });
  });

  // Files tab state
  let selectedConfigFile = $state('config/sodium-options.json');
  let configContent = $state(`{
  "quality": {
    "clouds": "FAST",
    "weather": "DEFAULT",
    "leaves": "SMART"
  },
  "performance": {
    "chunk_builder_threads": 8,
    "always_defer_chunk_updates": true,
    "use_block_face_culling": true
  }
}`);

  // Settings & Doctor tab state
  let preLaunchHook = $state('');
  let wrapperCmd = $state('');
  let doctorReport = $state<string[]>([
    'PASS: CAS hardlink integrity verified (84/84 jars match SHA-512)',
    'PASS: Java 25.0.1 LTS matches Minecraft 26.3 classfile version 69.0',
    'PASS: No duplicate mod slugs or Mixin conflicts detected',
  ]);

  function runDryRunUpgrade() {
    dryRunReport = `Dry-Run Upgrade Preview (${instance.mcVersion} ${instance.loader} -> ${targetVersion} ${targetLoader}): 81/84 mods compatible, 3 mods have automatic 26.x updates queued, snapshot #${historySnapshotCount + 1} will be saved to .history/.`;
  }

  function toggleMod(modId: string) {
    mods = mods.map((m) => (m.id === modId ? { ...m, enabled: !m.enabled } : m));
  }

  function applyBulkModUpdate() {
    historySnapshotCount += 1;
    mods = mods.map((m) => (m.hasUpdate ? { ...m, version: m.latestVersion, hasUpdate: false } : m));
    onNotify(`Updated all outdated mods. Snapshot #${historySnapshotCount} saved to .history/ for 1-click rollback.`);
  }

  function rollbackModHistory() {
    onNotify(`Rolled back instance mod state to .history/snapshot-${historySnapshotCount}.json`);
  }

  function triggerWorldLaunch(world: WorldRecord) {
    if (world.dataVersion > instance.dataVersion) {
      downgradeWarningWorld = world;
      return;
    }
    onLaunch(`${instance.name} -> World: ${world.name} (--quickPlaySingleplayer)`);
  }
</script>

<section class="instance-dashboard" data-testid="instance-dashboard" style="--accent: {instance.accentColor}">
  <div class="hero-banner">
    <div>
      <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px;">
        <span class="badge badge-success">{instance.mcVersion}</span>
        <span class="badge badge-info">{instance.loader.toUpperCase()} {instance.loaderVersion}</span>
        <span class="badge">Java {instance.javaMajor}</span>
        <span class="badge tabular">DataVersion {instance.dataVersion}</span>
      </div>
      <h1 style="margin:0;font-size:1.5rem;">{instance.name}</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;margin-top:4px;">
        Playtime: <strong class="tabular">{Math.floor(instance.playtimeMinutes / 60)}h {instance.playtimeMinutes % 60}m</strong>
        • Active Profile: <strong>{instance.activeProfile}</strong>
        • Last Played: {instance.lastPlayedLabel}
      </div>
    </div>
    <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap;">
      <button class="btn btn-primary" onclick={() => onLaunch(instance.name)} data-testid="instance-play-btn">
        ▶ Play Instance
      </button>
      <button class="btn" onclick={() => onCloneInstance(instance)} data-testid="instance-clone-btn">
        ⧉ Instant Reflink Clone
      </button>
      <button class="btn" onclick={() => onNotify(`Exported ${instance.name}.mrpack (with provenance hashes)`)}>
        ⤓ Export .mrpack
      </button>
    </div>
  </div>

  <!-- 11-Tab Navigation Bar -->
  <div class="tabs-bar" role="tablist" aria-label="Instance Dashboard Tabs">
    {#each TABS as tab}
      <button
        role="tab"
        aria-selected={activeTab === tab}
        class="tab-btn {activeTab === tab ? 'active' : ''}"
        data-testid="instance-tab-{tab.toLowerCase().replace(/\s+/g, '-')}"
        onclick={() => (activeTab = tab)}
      >
        {tab}
        {#if tab === 'Mods'}
          <span class="badge tabular" style="margin-left:4px;">{mods.length}</span>
        {/if}
      </button>
    {/each}
  </div>

  {#if activeTab === 'Overview'}
    <div class="grid-cards">
      <div class="card">
        <h3 style="margin-top:0;">Version & Loader Switcher (Dry-Run)</h3>
        <p style="color:var(--text-secondary);font-size:0.83rem;">
          Preview compatibility before switching Minecraft version or modloader.
        </p>
        <div style="display:flex;gap:8px;margin-bottom:10px;flex-wrap:wrap;">
          <select class="select" bind:value={targetVersion} aria-label="Target Minecraft Version">
            <option value="26.3">26.3 (Latest Release)</option>
            <option value="26.4-snapshot-4">26.4-snapshot-4</option>
            <option value="1.21.4">1.21.4</option>
            <option value="1.20.1">1.20.1</option>
          </select>
          <select class="select" bind:value={targetLoader} aria-label="Target Loader">
            <option value="fabric">Fabric</option>
            <option value="neoforge">NeoForge</option>
            <option value="quilt">Quilt</option>
            <option value="forge">Forge</option>
            <option value="vanilla">Vanilla</option>
          </select>
          <button class="btn btn-primary" onclick={runDryRunUpgrade}>Run Dry-Run Check</button>
        </div>
        {#if dryRunReport}
          <div class="badge badge-info" style="display:block;padding:8px;border-radius:6px;white-space:normal;">
            {dryRunReport}
          </div>
        {/if}
      </div>

      <div class="card">
        <h3 style="margin-top:0;">Instance Notes & CAS Footprint</h3>
        <p style="color:var(--text-secondary);font-size:0.84rem;">
          Content-Addressed Storage (`data/cas/sha256/`) deduplicates <strong>94.2%</strong> of mod & asset blocks via hardlinks/reflinks.
        </p>
        <div style="display:flex;gap:8px;flex-wrap:wrap;">
          <span class="badge badge-success">Unique Instance Delta: 18.4 MB</span>
          <span class="badge">Shared CAS Pool: 1.42 GB</span>
        </div>
      </div>
    </div>
  {:else if activeTab === 'Mods'}
    <div class="card" style="display:flex;flex-direction:column;gap:12px;">
      <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;">
        <div style="display:flex;align-items:center;gap:8px;flex:1;min-width:240px;">
          <input
            class="input"
            style="flex:1;"
            type="search"
            placeholder="Filter {mods.length} mods (<16ms indexed search)..."
            bind:value={modSearch}
            data-testid="mods-search-input"
            aria-label="Filter mods"
          />
          <span class="badge tabular" data-testid="mods-match-count">{filteredMods.length} / {mods.length}</span>
        </div>

        <div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap;">
          <span style="font-size:0.78rem;color:var(--text-muted);">Mod Profile:</span>
          {#each ['Default', 'Performance', 'Full'] as const as prof}
            <button
              class="btn btn-sm {activeModProfile === prof ? 'btn-primary' : ''}"
              onclick={() => (activeModProfile = prof)}
            >
              {prof}
            </button>
          {/each}
          <button class="btn btn-sm" onclick={() => (showProfileDiff = !showProfileDiff)} data-testid="profile-diff-btn">
            {showProfileDiff ? 'Hide Diff' : 'Diff Profiles'}
          </button>
          <button class="btn btn-sm btn-primary" onclick={applyBulkModUpdate} data-testid="bulk-update-mods-btn">
            ↻ Update All
          </button>
          <button class="btn btn-sm" onclick={rollbackModHistory} data-testid="rollback-mods-btn">
            ⟲ Rollback (.history #{historySnapshotCount})
          </button>
        </div>
      </div>

      {#if showProfileDiff}
        <div class="card" style="background:var(--bg-elevated);padding:10px;" data-testid="profile-diff-panel">
          <strong>Mod Profile Diff (`Performance` vs `Full`):</strong>
          <div style="font-size:0.8rem;color:var(--text-secondary);margin-top:4px;">
            + Enabled in Performance: <code>Sodium</code>, <code>Lithium</code>, <code>FerriteCore</code>, <code>ImmediatelyFast</code>
            • − Disabled Heavy Visuals: <code>Distant Horizons LOD</code>, <code>Sound Physics Remastered</code> (saves ~1.1 GB RAM)
          </div>
        </div>
      {/if}

      <!-- Virtualized 2,000-Mod List Viewport -->
      <div
        class="mods-virtual-viewport"
        data-testid="mods-virtual-viewport"
        style="height:420px;overflow-y:auto;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);position:relative;"
        onscroll={(e) => (modScrollTop = (e.currentTarget as HTMLElement).scrollTop)}
      >
        <div style="height:{totalModsHeight}px;position:relative;">
          <div style="transform:translateY({startIdx * ROW_HEIGHT}px);">
            {#each visibleMods as mod (mod.id)}
              <div
                class="mod-row"
                data-testid="mod-row"
                style="height:{ROW_HEIGHT}px;display:flex;align-items:center;justify-content:space-between;padding:0 12px;border-bottom:1px solid var(--border-subtle);"
              >
                <div style="display:flex;align-items:center;gap:10px;min-width:0;">
                  <input
                    type="checkbox"
                    checked={mod.enabled}
                    onchange={() => toggleMod(mod.id)}
                    aria-label="Enable {mod.name}"
                  />
                  <strong style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">{mod.name}</strong>
                  <span class="badge mono">{mod.version}</span>
                  <span
                    class="badge {mod.provider === 'modrinth'
                      ? 'badge-success'
                      : mod.provider === 'curseforge'
                        ? 'badge-warn'
                        : 'badge-info'}"
                  >
                    {mod.provider}
                  </span>
                  {#if mod.hasUpdate}
                    <span class="badge badge-info">Update -> {mod.latestVersion}</span>
                  {/if}
                </div>
                <div style="display:flex;align-items:center;gap:8px;">
                  <span class="badge tabular">{mod.sizeKb} KB</span>
                  <span class="badge">{mod.environment}</span>
                </div>
              </div>
            {/each}
          </div>
        </div>
      </div>
    </div>
  {:else if activeTab === 'Worlds'}
    <div class="card" style="display:flex;flex-direction:column;gap:12px;">
      <div style="display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:8px;">
        <div>
          <h3 style="margin:0;">Worlds (`level.dat` & `DataVersion` Guard)</h3>
          <span style="font-size:0.82rem;color:var(--text-secondary);">
            Automatic World Downgrade Guard blocks accidental corruption when opening newer worlds in older Minecraft versions.
          </span>
        </div>
        <button
          class="btn btn-sm badge-warn"
          data-testid="test-downgrade-guard-btn"
          onclick={() => (downgradeWarningWorld = worlds[1] ?? worlds[0])}
        >
          ⚠ Simulate World Downgrade Guard
        </button>
      </div>

      <table class="data-table">
        <thead>
          <tr>
            <th>World Name</th>
            <th>MC Version / DataVersion</th>
            <th>Mode & Difficulty</th>
            <th>Size</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {#each worlds as world (world.id)}
            <tr>
              <td><strong>{world.name}</strong><br /><span class="mono" style="font-size:0.75rem;color:var(--text-muted);">{world.folderName}</span></td>
              <td><span class="badge">{world.mcVersionName}</span> <span class="badge tabular">DV {world.dataVersion}</span></td>
              <td>{world.gameMode} • {world.difficulty}</td>
              <td class="tabular">{world.sizeMb} MB</td>
              <td style="display:flex;gap:6px;flex-wrap:wrap;">
                <button class="btn btn-sm btn-primary" onclick={() => triggerWorldLaunch(world)}>▶ Quick Play</button>
                <button class="btn btn-sm" onclick={() => onNotify(`Created zstd backup: backups/${world.folderName}-20261016.tar.zst`)}>
                  ⛁ zstd Backup
                </button>
                <button
                  class="btn btn-sm"
                  onclick={() => {
                    world.pinnedToHome = !world.pinnedToHome;
                    onNotify(world.pinnedToHome ? `Pinned ${world.name} to Home` : `Unpinned ${world.name}`);
                  }}
                >
                  {world.pinnedToHome ? '★ Pinned' : '☆ Pin'}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if activeTab === 'Servers'}
    <div class="card">
      <h3 style="margin-top:0;">Saved Servers (`servers.dat` NBT + Live SLP Ping)</h3>
      <table class="data-table">
        <thead>
          <tr>
            <th>Server</th>
            <th>Address</th>
            <th>MOTD</th>
            <th>Players</th>
            <th>Ping</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {#each servers as srv (srv.id)}
            <tr>
              <td><strong>{srv.name}</strong></td>
              <td class="mono">{srv.address}</td>
              <td style="color:var(--text-secondary);">{srv.motd}</td>
              <td class="tabular">{srv.playersOnline} / {srv.playersMax}</td>
              <td><span class="badge badge-success tabular">{srv.pingMs} ms</span></td>
              <td style="display:flex;gap:6px;">
                <button class="btn btn-sm btn-primary" onclick={() => onLaunch(`${instance.name} -> Server ${srv.address}`)}>
                  ▶ Join Server
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if activeTab === 'Logs'}
    <div class="card" style="display:flex;flex-direction:column;gap:10px;">
      <div style="display:flex;align-items:center;justify-content:space-between;gap:8px;flex-wrap:wrap;">
        <div style="display:flex;gap:8px;align-items:center;">
          <input class="input" placeholder="Search or regex filter logs..." bind:value={logFilter} aria-label="Search logs" />
          <select class="select" bind:value={logLevel} aria-label="Log severity filter">
            <option value="ALL">ALL LEVELS</option>
            <option value="INFO">INFO</option>
            <option value="WARN">WARN</option>
            <option value="ERROR">ERROR</option>
          </select>
        </div>
        <div style="display:flex;gap:8px;">
          <span class="badge badge-success tabular">CPU: 14.2%</span>
          <span class="badge badge-info tabular">RSS: 2,410 / {instance.ramMb} MB</span>
          <button
            class="btn btn-sm"
            data-testid="copy-redacted-logs-btn"
            onclick={() => {
              const redacted = ipcAdapter.redactSensitiveLog(rawLogs.join('\n'));
              onNotify(`Copied redacted log (${redacted.includes('[REDACTED_ACCESS_TOKEN]') ? 'Tokens stripped' : 'Clean'})`);
            }}
          >
            📋 Copy Redacted Log
          </button>
        </div>
      </div>
      <pre
        style="background:var(--bg-input);padding:12px;border-radius:var(--radius-sm);border:1px solid var(--border-subtle);max-height:280px;overflow:auto;margin:0;font-size:0.78rem;"
      >{filteredLogs.map((l) => ipcAdapter.redactSensitiveLog(l)).join('\n')}</pre>
    </div>
  {:else if activeTab === 'Files'}
    <div class="card" style="display:flex;flex-direction:column;gap:10px;">
      <div style="display:flex;align-items:center;justify-content:space-between;">
        <div style="display:flex;gap:8px;align-items:center;">
          <strong>Safe Config Editor:</strong>
          <select class="select" bind:value={selectedConfigFile} aria-label="Select config file">
            <option value="config/sodium-options.json">config/sodium-options.json</option>
            <option value="options.txt">options.txt</option>
            <option value="config/iris.properties">config/iris.properties</option>
          </select>
        </div>
        <button class="btn btn-sm btn-primary" onclick={() => onNotify(`Saved ${selectedConfigFile} with syntax check`)}>
          Save Config
        </button>
      </div>
      <textarea
        class="input mono"
         rows="9"
        bind:value={configContent}
        aria-label="Configuration file editor"
        style="width:100%;font-size:0.8rem;"
      ></textarea>
    </div>
  {:else if activeTab === 'Settings'}
    <div class="grid-cards">
      <div class="card">
        <h3 style="margin-top:0;">Per-Instance Runtime & Hooks</h3>
        <div style="display:flex;flex-direction:column;gap:8px;">
          <label style="font-size:0.82rem;">
            Allocated Heap RAM (MB): <strong class="tabular">{instance.ramMb} MB</strong>
            <input type="range" min="1024" max="16384" step="512" bind:value={instance.ramMb} style="width:100%;" />
          </label>
          <label style="font-size:0.82rem;">
            Pre-Launch Hook Command:
            <input class="input" style="width:100%;margin-top:4px;" placeholder="e.g. ./scripts/sync-configs.sh" bind:value={preLaunchHook} />
          </label>
          <label style="font-size:0.82rem;">
            Wrapper Command (MangoHud / GameMode):
            <input class="input" style="width:100%;margin-top:4px;" placeholder="e.g. mangohud --dlsym gamemoderun" bind:value={wrapperCmd} />
          </label>
        </div>
      </div>

      <div class="card" data-testid="instance-doctor-card">
        <h3 style="margin-top:0;">Instance Doctor (Self-Repair)</h3>
        <ul style="padding-left:18px;font-size:0.83rem;color:var(--text-secondary);">
          {#each doctorReport as item}
            <li style="margin-bottom:4px;">{item}</li>
          {/each}
        </ul>
        <button
          class="btn btn-primary btn-sm"
          onclick={() => onNotify('Instance Doctor verified 100% of libraries, natives, and CAS links.')}
        >
          🩺 Run Full Integrity & Repair Scan
        </button>
      </div>
    </div>
  {:else}
    <div class="card">
      <h3 style="margin-top:0;">{activeTab} Manager — {instance.name}</h3>
      <p style="color:var(--text-secondary);font-size:0.85rem;">
        Content-addressed {activeTab.toLowerCase()} directory linked to <code>data/instances/{instance.id}/</code>. Drag and drop archives anywhere to install.
      </p>
      <div style="display:flex;gap:8px;">
        <button class="btn btn-primary btn-sm" onclick={() => onNotify(`Opened ${activeTab} folder`)}>
          Open {activeTab} Folder
        </button>
        <button class="btn btn-sm" onclick={() => onNotify(`Scanned ${activeTab} metadata`)}>
          Rescan Directory
        </button>
      </div>
    </div>
  {/if}

  {#if downgradeWarningWorld}
    <div class="modal-backdrop" data-testid="world-downgrade-modal">
      <div class="modal-panel">
        <div style="display:flex;align-items:center;gap:8px;">
          <span class="badge badge-danger">WORLD DOWNGRADE GUARD</span>
          <h3 style="margin:0;">Prevent Save Corruption: {downgradeWarningWorld.name}</h3>
        </div>
        <p style="color:var(--text-secondary);font-size:0.88rem;margin:0;">
          This world was last saved in <strong>Minecraft {downgradeWarningWorld.mcVersionName}</strong>
          (<code>DataVersion {downgradeWarningWorld.dataVersion}</code>). Opening it in an older version
          (<code>{simulatedTargetDowngradeVersion}</code>) can permanently wipe chunks and inventories.
        </p>
        <div style="display:flex;justify-content:flex-end;gap:8px;">
          <button class="btn" onclick={() => (downgradeWarningWorld = null)}>Cancel Launch (Recommended)</button>
          <button
            class="btn btn-primary"
            onclick={() => {
              const w = downgradeWarningWorld!;
              downgradeWarningWorld = null;
              onNotify(`Created automatic zstd backup of ${w.name} before launch.`);
              onLaunch(`${instance.name} -> ${w.name} (Backed up)`);
            }}
          >
            Backup (.tar.zst) & Launch Anyway
          </button>
        </div>
      </div>
    </div>
  {/if}
</section>
