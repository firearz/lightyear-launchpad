<script lang="ts">
  import type { DiscoverItemRecord, InstanceRecord } from '../lib/ipc/fixtures';

  interface Props {
    catalog: DiscoverItemRecord[];
    targetInstance: InstanceRecord;
    onNotify: (msg: string) => void;
  }

  let { catalog, targetInstance, onNotify }: Props = $props();

  let searchQuery = $state('');
  let selectedCategory = $state<'All' | 'Mod' | 'Modpack' | 'Resource Pack' | 'Shader'>('All');
  let selectedProvider = $state<'all' | 'modrinth' | 'curseforge'>('all');
  let autoFilterInstance = $state(true);

  let solverPreviewItem = $state<DiscoverItemRecord | null>(null);
  let restrictedModalItem = $state<DiscoverItemRecord | null>(null);

  let filteredCatalog = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return catalog.filter((item) => {
      if (selectedCategory !== 'All' && item.category !== selectedCategory) return false;
      if (selectedProvider !== 'all' && item.provider !== selectedProvider) return false;
      if (autoFilterInstance && !item.mcVersions.includes(targetInstance.mcVersion)) return false;
      if (q && !`${item.title} ${item.summary} ${item.author}`.toLowerCase().includes(q)) return false;
      return true;
    });
  });

  function handleInstallClick(item: DiscoverItemRecord) {
    if (item.restrictedManualDownload) {
      restrictedModalItem = item;
      return;
    }
    solverPreviewItem = item;
  }
</script>

<section class="discover-view" data-testid="discover-view">
  <div class="hero-banner">
    <div>
      <span class="badge badge-info">UNIFIED CONTENT HUB</span>
      <h1 style="margin:6px 0 4px;font-size:1.45rem;">Discover Mods, Modpacks, Shaders & Resource Packs</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;">
        Auto-filtered for target instance: <strong>{targetInstance.name}</strong>
        (<code>{targetInstance.mcVersion}</code> • <code>{targetInstance.loader}</code>)
      </div>
    </div>
    <label style="display:flex;align-items:center;gap:8px;font-size:0.84rem;cursor:pointer;">
      <input type="checkbox" bind:checked={autoFilterInstance} />
      Auto-filter to {targetInstance.mcVersion} ({targetInstance.loader})
    </label>
  </div>

  <div class="card" style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;">
    <input
      class="input"
      style="flex:1;min-width:220px;"
      type="search"
      placeholder="Search Modrinth & CurseForge simultaneously..."
      bind:value={searchQuery}
      data-testid="discover-search-input"
      aria-label="Search Modrinth and CurseForge"
    />
    <div style="display:flex;gap:6px;flex-wrap:wrap;">
      {#each ['All', 'Mod', 'Modpack', 'Resource Pack', 'Shader'] as const as cat}
        <button
          class="btn btn-sm {selectedCategory === cat ? 'btn-primary' : ''}"
          onclick={() => (selectedCategory = cat)}
        >
          {cat}
        </button>
      {/each}
    </div>
    <select class="select" bind:value={selectedProvider} aria-label="Filter provider">
      <option value="all">Modrinth + CurseForge</option>
      <option value="modrinth">Modrinth Only</option>
      <option value="curseforge">CurseForge Only</option>
    </select>
  </div>

  <div class="grid-cards">
    {#each filteredCatalog as item (item.id)}
      <div class="card" style="display:flex;flex-direction:column;justify-content:space-between;gap:12px;" data-testid="discover-card-{item.id}">
        <div>
          <div style="display:flex;align-items:center;justify-content:space-between;gap:6px;margin-bottom:6px;">
            <span class="badge {item.provider === 'modrinth' ? 'badge-success' : 'badge-warn'}">
              {item.provider.toUpperCase()}
            </span>
            <span class="badge tabular">⬇ {item.downloads}</span>
          </div>
          <h3 style="margin:0 0 4px;font-size:1.02rem;">{item.title}</h3>
          <div style="font-size:0.76rem;color:var(--text-muted);margin-bottom:8px;">
            by {item.author} • {item.category}
          </div>
          <p style="margin:0;font-size:0.83rem;color:var(--text-secondary);">
            {item.summary}
          </p>
        </div>

        <div style="display:flex;flex-direction:column;gap:8px;">
          <div style="display:flex;gap:4px;flex-wrap:wrap;">
            {#each item.dependencies as dep}
              <span class="badge mono">dep: {dep}</span>
            {/each}
          </div>
          <button
            class="btn {item.restrictedManualDownload ? '' : 'btn-primary'}"
            data-testid="install-{item.id}-btn"
            onclick={() => handleInstallClick(item)}
          >
            {item.restrictedManualDownload ? '⤓ Guided Browser Download (Opt-Out)' : '+ Install with Dependency Solver'}
          </button>
        </div>
      </div>
    {/each}
  </div>

  {#if solverPreviewItem}
    <div class="modal-backdrop" data-testid="dependency-solver-modal">
      <div class="modal-panel">
        <div style="display:flex;align-items:center;gap:8px;">
          <span class="badge badge-success">SAT DEPENDENCY SOLVER</span>
          <h3 style="margin:0;">Install Plan for {solverPreviewItem.title}</h3>
        </div>
        <p style="margin:0;font-size:0.86rem;color:var(--text-secondary);">
          Target Instance: <strong>{targetInstance.name}</strong> ({targetInstance.mcVersion} {targetInstance.loader}).
          All artifacts will be verified by SHA-512/SHA-1 and hardlinked from the global CAS store.
        </p>
        <div class="card" style="background:var(--bg-input);">
          <div><strong>Primary Package:</strong> {solverPreviewItem.title} (latest for {targetInstance.mcVersion})</div>
          <div style="margin-top:6px;">
            <strong>Auto-Resolved Required Dependencies ({solverPreviewItem.dependencies.length}):</strong>
            <ul style="margin:4px 0 0;padding-left:18px;">
              {#each solverPreviewItem.dependencies as dep}
                <li class="mono">{dep} (compatible version verified — no conflicts)</li>
              {/each}
            </ul>
          </div>
        </div>
        <div style="display:flex;justify-content:flex-end;gap:8px;">
          <button class="btn" onclick={() => (solverPreviewItem = null)}>Cancel</button>
          <button
            class="btn btn-primary"
            onclick={() => {
              const title = solverPreviewItem!.title;
              solverPreviewItem = null;
              onNotify(`Installed ${title} + required dependencies into ${targetInstance.name}`);
            }}
          >
            Confirm & Link to Instance
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if restrictedModalItem}
    <div class="modal-backdrop" data-testid="curseforge-restricted-modal">
      <div class="modal-panel">
        <div style="display:flex;align-items:center;gap:8px;">
          <span class="badge badge-warn">CURSEFORGE THIRD-PARTY OPT-OUT</span>
          <h3 style="margin:0;">Guided Download Watcher: {restrictedModalItem.title}</h3>
        </div>
        <p style="margin:0;font-size:0.86rem;color:var(--text-secondary);">
          The author of <strong>{restrictedModalItem.title}</strong> has disabled direct third-party launcher downloads via the CurseForge API.
          Kestryx respects creator opt-out settings and watches your <code>Downloads/</code> directory to auto-verify the expected SHA-1 hash and link it into your instance.
        </p>
        <div style="display:flex;justify-content:flex-end;gap:8px;">
          <button class="btn" onclick={() => (restrictedModalItem = null)}>Close</button>
          <button
            class="btn btn-primary"
            onclick={() => {
              const title = restrictedModalItem!.title;
              restrictedModalItem = null;
              onNotify(`Simulated Downloads/ watcher capture & SHA-1 verification for ${title}`);
            }}
          >
            Simulate Browser Download & Auto-Import
          </button>
        </div>
      </div>
    </div>
  {/if}
</section>
