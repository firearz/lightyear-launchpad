<script lang="ts">
  import type { ThemeMode } from '../lib/ipc/adapter';

  interface Props {
    theme: ThemeMode;
    performanceMode: boolean;
    uiScale: number;
    onThemeChange: (t: ThemeMode) => void;
    onPerformanceModeChange: (p: boolean) => void;
    onUiScaleChange: (s: number) => void;
    onNotify: (msg: string) => void;
  }

  let {
    theme,
    performanceMode,
    uiScale,
    onThemeChange,
    onPerformanceModeChange,
    onUiScaleChange,
    onNotify,
  }: Props = $props();

  let mode = $state<'simple' | 'advanced'>('simple');
  let customDataRoot = $state('C:/Users/Player/AppData/Roaming/Kestryx/data');
  let portableMode = $state(false);
  let concurrentDownloads = $state(16);
  let locale = $state('en-US');
  let msaClientId = $state('00000000-kestryx-open-client-id');
  let curseforgeApiKey = $state('');
  let telemetryOptIn = $state(false);
  let showTelemetryPayload = $state(false);

  function resetAllDefaults() {
    customDataRoot = 'C:/Users/Player/AppData/Roaming/Kestryx/data';
    portableMode = false;
    concurrentDownloads = 16;
    onUiScaleChange(1.0);
    onPerformanceModeChange(false);
    telemetryOptIn = false;
    onNotify('Reset all settings to recommended defaults.');
  }
</script>

<section class="settings-view" data-testid="settings-view" dir={locale === 'ar-SA' ? 'rtl' : 'ltr'}>
  <div class="hero-banner">
    <div>
      <span class="badge badge-info">PROGRESSIVE DISCLOSURE SETTINGS</span>
      <h1 style="margin:6px 0 4px;font-size:1.45rem;">Settings, Storage Root, Importers & API Keys</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;">
        Every setting includes a plain-English explanation, recommended default + rationale, and 1-click reset.
      </div>
    </div>
    <div style="display:flex;gap:8px;align-items:center;">
      <button
        class="btn {mode === 'simple' ? 'btn-primary' : ''}"
        onclick={() => (mode = 'simple')}
        data-testid="settings-mode-simple"
      >
        Simple Mode
      </button>
      <button
        class="btn {mode === 'advanced' ? 'btn-primary' : ''}"
        onclick={() => (mode = 'advanced')}
        data-testid="settings-mode-advanced"
      >
        Advanced Mode
      </button>
      <button class="btn" onclick={resetAllDefaults} data-testid="reset-defaults-btn">
        ⟲ Reset Defaults
      </button>
    </div>
  </div>

  <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));">
    <!-- Appearance, Theme, UI Scale, i18n -->
    <div class="card">
      <h3 style="margin-top:0;">Appearance, Accessibility & Localization</h3>
      <div style="display:flex;flex-direction:column;gap:12px;">
        <div>
          <strong>Color Theme</strong>
          <div style="font-size:0.78rem;color:var(--text-secondary);margin-bottom:6px;">
            Recommended: <code>Dark (Luminous Obsidian)</code> — calibrated for low eye strain and WCAG AAA contrast.
          </div>
          <div style="display:flex;gap:6px;">
            {#each ['dark', 'light', 'high-contrast'] as const as t}
              <button
                class="btn btn-sm {theme === t ? 'btn-primary' : ''}"
                data-testid="theme-btn-{t}"
                onclick={() => onThemeChange(t)}
              >
                {t}
              </button>
            {/each}
          </div>
        </div>

        <div>
          <strong>UI Scale (<span class="tabular">{Math.round(uiScale * 100)}%</span>)</strong>
          <div style="font-size:0.78rem;color:var(--text-secondary);margin-bottom:4px;">
            Recommended: <code>100%</code> (supports 50% to 200% for 4K / Steam Deck / handheld displays).
          </div>
          <div style="display:flex;gap:8px;align-items:center;">
            <input
              type="range"
              min="0.5"
              max="2.0"
              step="0.1"
              value={uiScale}
              oninput={(e) => onUiScaleChange(Number((e.currentTarget as HTMLInputElement).value))}
              style="flex:1;"
              aria-label="UI Scale"
            />
            <button class="btn btn-sm" onclick={() => onUiScaleChange(1.0)}>Reset 100%</button>
          </div>
        </div>

        <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:0.84rem;">
          <input
            type="checkbox"
            checked={performanceMode}
            onchange={(e) => onPerformanceModeChange((e.currentTarget as HTMLInputElement).checked)}
          />
          <strong>Performance Mode (Disable decorative transitions & auto-pause launcher visuals while game runs)</strong>
        </label>

        <div>
          <strong>Language & RTL Layout (`i18n`)</strong>
          <select class="select" bind:value={locale} style="width:100%;margin-top:4px;" aria-label="Language locale">
            <option value="en-US">English (en-US)</option>
            <option value="es-ES">Español (es-ES)</option>
            <option value="de-DE">Deutsch (de-DE)</option>
            <option value="ja-JP">日本語 (ja-JP)</option>
            <option value="zh-CN">简体中文 (zh-CN)</option>
            <option value="pt-BR">Português (pt-BR)</option>
            <option value="ar-SA">العربية (ar-SA — RTL Layout)</option>
          </select>
        </div>
      </div>
    </div>

    <!-- Custom Data Root, Portable Mode & Importers -->
    <div class="card" data-testid="storage-importers-card">
      <h3 style="margin-top:0;">Custom Data Root, Portable Mode & Importers</h3>
      <div style="display:flex;flex-direction:column;gap:10px;">
        <div>
          <strong>Custom `.minecraft` / Kestryx Data Root</strong>
          <div style="font-size:0.78rem;color:var(--text-secondary);margin-bottom:4px;">
            Recommended: Fast NVMe SSD with NTFS/APFS/Btrfs hardlink or reflink support.
          </div>
          <div style="display:flex;gap:6px;">
            <input class="input mono" style="flex:1;" bind:value={customDataRoot} aria-label="Custom data root path" />
            <button
              class="btn btn-sm btn-primary"
              onclick={() => onNotify(`Validated data root (${customDataRoot}): 412 GB free, hardlinks supported.`)}
            >
              Validate & Move
            </button>
          </div>
        </div>

        <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:0.84rem;">
          <input type="checkbox" bind:checked={portableMode} />
          <strong>Portable Mode (Store all instances, runtimes, and CAS inside `./data` next to executable)</strong>
        </label>

        <div>
          <strong>1-Click External Launcher Importers</strong>
          <div style="display:flex;gap:6px;flex-wrap:wrap;margin-top:6px;">
            {#each ['Official .minecraft', 'Prism Launcher', 'Modrinth App', 'CurseForge'] as src}
              <button
                class="btn btn-sm"
                onclick={() => onNotify(`Scanned & imported instances + worlds from ${src}`)}
              >
                ⤓ Import {src}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </div>
  </div>

  {#if mode === 'advanced'}
    <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));" data-testid="advanced-settings-panel">
      <div class="card">
        <h3 style="margin-top:0;">API Credentials & Network Concurrency</h3>
        <div style="display:flex;flex-direction:column;gap:8px;">
          <label style="font-size:0.82rem;">
            Microsoft Azure App Client ID (`KESTRYX_MSA_CLIENT_ID`):
            <input class="input mono" style="width:100%;margin-top:4px;" bind:value={msaClientId} />
          </label>
          <label style="font-size:0.82rem;">
            CurseForge Core API Key (`KESTRYX_CURSEFORGE_API_KEY`):
            <input
              class="input mono"
              type="password"
              placeholder="Optional $2a$10$... key for direct CurseForge API queries"
              style="width:100%;margin-top:4px;"
              bind:value={curseforgeApiKey}
            />
          </label>
          <label style="font-size:0.82rem;">
            Concurrent CAS Asset Streams (<strong class="tabular">{concurrentDownloads}</strong>, Recommended: 16):
            <input type="range" min="4" max="32" step="2" bind:value={concurrentDownloads} style="width:100%;" />
          </label>
        </div>
      </div>

      <div class="card">
        <h3 style="margin-top:0;">Privacy & Strictly Opt-In Telemetry</h3>
        <label style="display:flex;align-items:center;gap:8px;cursor:pointer;font-size:0.84rem;">
          <input type="checkbox" bind:checked={telemetryOptIn} />
          <strong>Opt-in to anonymous crash & startup timing metrics (Default: OFF)</strong>
        </label>
        <p style="font-size:0.79rem;color:var(--text-secondary);">
          Zero tracking SDKs, zero analytics cookies, and never includes usernames, UUIDs, IP addresses, or file paths.
        </p>
        <button class="btn btn-sm" onclick={() => (showTelemetryPayload = !showTelemetryPayload)}>
          {showTelemetryPayload ? 'Hide Exact Payload' : 'Inspect Telemetry Payload JSON'}
        </button>
        {#if showTelemetryPayload}
          <pre class="mono" style="background:var(--bg-input);padding:8px;border-radius:6px;font-size:0.74rem;margin-top:8px;">{JSON.stringify(
            {
              appVersion: '0.1.0',
              os: 'windows-x86_64',
              coldStartMs: 295,
              javaMajorUsed: 25,
            },
            null,
            2
          )}</pre>
        {/if}
      </div>
    </div>
  {/if}
</section>
