<script lang="ts">
  const STATES = ['default', 'hover', 'pressed', 'focus', 'disabled', 'loading', 'empty', 'error'] as const;
</script>

<section class="gallery-view" data-testid="gallery-view">
  <div class="hero-banner">
    <div>
      <span class="badge badge-info">DESIGN SYSTEM VERIFICATION HARNESS</span>
      <h1 style="margin:6px 0 4px;font-size:1.45rem;">Kestryx `/__gallery` Component & 8-State Matrix</h1>
      <div style="color:var(--text-secondary);font-size:0.84rem;">
        Renders every core component across <code>default</code>, <code>hover</code>, <code>pressed</code>, <code>focus</code>, <code>disabled</code>, <code>loading</code>, <code>empty</code>, and <code>error</code> states.
      </div>
    </div>
  </div>

  <div class="card">
    <h3 style="margin-top:0;">1. Interactive Controls & Action States (8 States)</h3>
    <div class="grid-cards" style="grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));">
      {#each STATES as st}
        <div class="card" style="background:var(--bg-elevated);padding:12px;" data-testid="gallery-state-{st}">
          <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
            <span class="badge mono">{st.toUpperCase()}</span>
            <span class="tabular" style="font-size:0.74rem;color:var(--text-muted);">State Cell</span>
          </div>
          {#if st === 'disabled'}
            <button class="btn btn-primary" disabled style="width:100%;">Play 26.3 (Disabled)</button>
          {:else if st === 'loading'}
            <button class="btn btn-primary" style="width:100%;">⟳ Verifying CAS (78%)...</button>
          {:else if st === 'error'}
            <button class="btn btn-danger" style="width:100%;">✖ Hash Mismatch Retry</button>
          {:else if st === 'empty'}
            <div style="font-size:0.78rem;color:var(--text-muted);text-align:center;padding:6px;border:1px dashed var(--border-strong);border-radius:6px;">
              No items in slot
            </div>
          {:else}
            <button
              class="btn btn-primary"
              style="width:100%;{st === 'focus' ? 'outline:2px solid var(--accent-info);outline-offset:2px;' : st === 'pressed' ? 'transform:scale(0.98);' : ''}"
            >
              ▶ Launch 26.3 ({st})
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <div class="grid-cards" style="grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));">
    <div class="card">
      <h3 style="margin-top:0;">2. Colorblind-Safe Status Badges (Icon + Label + Color)</h3>
      <div style="display:flex;gap:8px;flex-wrap:wrap;">
        <span class="badge badge-success">✓ Ready / Verified (Emerald)</span>
        <span class="badge badge-info">ℹ Update Available (Cyan)</span>
        <span class="badge badge-warn">⚠ Experimental / Beta (Amber)</span>
        <span class="badge badge-danger">✖ Conflict / Error (Rose)</span>
        <span class="badge tabular">26.4-snapshot-4 • DV 4340</span>
      </div>
    </div>

    <div class="card">
      <h3 style="margin-top:0;">3. Typography & Tabular Numerals</h3>
      <div style="display:flex;flex-direction:column;gap:4px;font-size:0.84rem;">
        <div>Sans Display: <strong>Luminous Obsidian 26.3 Fabric</strong></div>
        <div class="tabular">Tabular Progress: <code>1,000 instances • 2,000 mods • 42.8 MB/s • 18 ms</code></div>
        <div class="mono">Mono Digest: <code>b50ad385-829d-3141-a216-7e7d7539ba7f</code></div>
      </div>
    </div>
  </div>
</section>
