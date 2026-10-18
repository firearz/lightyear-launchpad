<script lang="ts">
  import type { InstanceRecord } from '../lib/ipc/fixtures';

  interface Props {
    onClose: () => void;
    onCreate: (inst: InstanceRecord) => void;
  }

  let { onClose, onCreate }: Props = $props();

  let wizardMode = $state<'quick' | 'step-by-step' | 'import'>('quick');
  let name = $state('Kestryx 26.3 Speedrun');
  let mcVersion = $state('26.3');
  let loader = $state<InstanceRecord['loader']>('fabric');
  let optimizationPreset = $state(true);

  function submitCreate() {
    const newInst: InstanceRecord = {
      id: `inst-custom-${Date.now()}`,
      name: name.trim() || `Minecraft ${mcVersion}`,
      mcVersion,
      loader,
      loaderVersion: loader === 'vanilla' ? 'none' : '0.16.10',
      javaMajor: mcVersion.startsWith('26.') ? 25 : mcVersion.startsWith('1.21') ? 21 : mcVersion.startsWith('1.20') ? 17 : 8,
      playtimeMinutes: 0,
      lastPlayedLabel: 'Just created',
      favorite: true,
      tags: [loader === 'vanilla' ? 'vanilla' : 'modded', 'recent', 'favorites'],
      accentColor: '#10B981',
      modCount: optimizationPreset && loader !== 'vanilla' ? 12 : 0,
      dataVersion: mcVersion.startsWith('26.') ? 4325 : 4189,
      activeProfile: optimizationPreset ? 'Performance' : 'Default',
      ramMb: 4096,
      searchKey: `${name} ${mcVersion} ${loader}`.toLowerCase(),
    };
    onCreate(newInst);
  }
</script>

<div class="modal-backdrop" data-testid="create-instance-modal">
  <div class="modal-panel">
    <div style="display:flex;align-items:center;justify-content:space-between;">
      <div>
        <span class="badge badge-success">INSTANCE CREATOR</span>
        <h3 style="margin:4px 0 0;">Create or Import Minecraft Instance</h3>
      </div>
      <div style="display:flex;gap:6px;">
        <button class="btn btn-sm {wizardMode === 'quick' ? 'btn-primary' : ''}" onclick={() => (wizardMode = 'quick')}>
          1-Screen Quick Create
        </button>
        <button class="btn btn-sm {wizardMode === 'step-by-step' ? 'btn-primary' : ''}" onclick={() => (wizardMode = 'step-by-step')}>
          3-Step Wizard
        </button>
        <button class="btn btn-sm {wizardMode === 'import' ? 'btn-primary' : ''}" onclick={() => (wizardMode = 'import')}>
          Import .mrpack / Zip
        </button>
      </div>
    </div>

    {#if wizardMode === 'import'}
      <div class="card" style="background:var(--bg-input);text-align:center;padding:24px;">
        <strong>Drop `.mrpack`, CurseForge `.zip`, or MultiMC/Prism archive here</strong>
        <p style="font-size:0.82rem;color:var(--text-secondary);">
          Automatically verifies SHA-512 / SHA-1 hashes and hardlinks shared jars into the global CAS store.
        </p>
        <button class="btn btn-primary" onclick={submitCreate}>Simulate Instant .mrpack Import</button>
      </div>
    {:else}
      <div style="display:flex;flex-direction:column;gap:10px;">
        <label style="font-size:0.84rem;">
          Instance Name:
          <input class="input" style="width:100%;margin-top:4px;" bind:value={name} data-testid="new-instance-name" />
        </label>

        <div style="display:grid;grid-template-columns:1fr 1fr;gap:10px;">
          <label style="font-size:0.84rem;">
            Minecraft Version (All Eras):
            <select class="select" style="width:100%;margin-top:4px;" bind:value={mcVersion} data-testid="new-instance-version">
              <option value="26.3">26.3 (Latest Year-Numbered Release)</option>
              <option value="26.4-snapshot-4">26.4-snapshot-4 (Snapshot)</option>
              <option value="1.21.4">1.21.4 (Release)</option>
              <option value="1.20.1">1.20.1 (Release)</option>
              <option value="1.16.5">1.16.5 (Release)</option>
              <option value="1.12.2">1.12.2 (Legacy Forge Era)</option>
              <option value="1.7.10">1.7.10 (Legacy Era)</option>
              <option value="b1.7.3">b1.7.3 (Golden Age Beta)</option>
              <option value="a1.2.6">a1.2.6 (Alpha)</option>
              <option value="c0.30_01c">c0.30_01c (Classic)</option>
            </select>
          </label>

          <label style="font-size:0.84rem;">
            Modloader:
            <select class="select" style="width:100%;margin-top:4px;" bind:value={loader} data-testid="new-instance-loader">
              <option value="fabric">Fabric</option>
              <option value="neoforge">NeoForge</option>
              <option value="quilt">Quilt</option>
              <option value="forge">Forge</option>
              <option value="vanilla">Vanilla (Unmodded)</option>
            </select>
          </label>
        </div>

        <label style="display:flex;align-items:center;gap:8px;font-size:0.84rem;cursor:pointer;margin-top:4px;">
          <input type="checkbox" bind:checked={optimizationPreset} />
          <strong>Include Curated Optimization Preset (`Sodium`, `Lithium`, `FerriteCore`, `EntityCulling`)</strong>
        </label>
      </div>
    {/if}

    <div style="display:flex;justify-content:flex-end;gap:8px;">
      <button class="btn" onclick={onClose}>Cancel</button>
      <button class="btn btn-primary" onclick={submitCreate} data-testid="confirm-create-instance-btn">
        ✓ Create Instance
      </button>
    </div>
  </div>
</div>
