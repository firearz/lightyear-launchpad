import { computeOfflinePlayerUuid } from './offlineUuid';

export type FixturePreset = 'empty' | 'typical' | 'stress';

export interface InstanceRecord {
  id: string;
  name: string;
  mcVersion: string;
  loader: 'fabric' | 'neoforge' | 'forge' | 'quilt' | 'vanilla';
  loaderVersion: string;
  javaMajor: 25 | 21 | 17 | 16 | 8;
  playtimeMinutes: number;
  lastPlayedLabel: string;
  favorite: boolean;
  tags: string[];
  accentColor: string;
  modCount: number;
  dataVersion: number;
  activeProfile: 'Default' | 'Performance' | 'Full';
  ramMb: number;
  searchKey: string;
}

export interface ModRecord {
  id: string;
  slug: string;
  name: string;
  version: string;
  latestVersion: string;
  hasUpdate: boolean;
  provider: 'modrinth' | 'curseforge' | 'local';
  enabled: boolean;
  sizeKb: number;
  environment: 'client+server' | 'client' | 'server';
  profiles: Array<'Default' | 'Performance' | 'Full'>;
  searchKey: string;
}

export interface WorldRecord {
  id: string;
  name: string;
  folderName: string;
  mcVersionName: string;
  dataVersion: number;
  gameMode: 'Survival' | 'Creative' | 'Hardcore' | 'Adventure';
  difficulty: 'Peaceful' | 'Easy' | 'Normal' | 'Hard';
  cheats: boolean;
  lastPlayed: string;
  sizeMb: number;
  pinnedToHome: boolean;
}

export interface ServerRecord {
  id: string;
  name: string;
  address: string;
  motd: string;
  playersOnline: number;
  playersMax: number;
  pingMs: number;
  versionTag: string;
  pinnedToHome: boolean;
}

export interface AccountRecord {
  id: string;
  username: string;
  type: 'microsoft' | 'offline';
  uuidDashed: string;
  uuidUndashed: string;
  active: boolean;
  skinVariant: 'classic' | 'slim';
  capeId: string;
  tokenStatus: 'valid' | 'refresh_needed' | 'offline_local';
}

export interface JavaRuntimeRecord {
  id: string;
  major: 25 | 21 | 17 | 16 | 8;
  vendor: string;
  versionString: string;
  path: string;
  arch: string;
  managed: boolean;
  supportsAotCache: boolean;
  supportsAppCds: boolean;
  status: 'Verified' | 'Ready';
}

export interface CrashSignatureRecord {
  id: string;
  code: string;
  title: string;
  category:
    | 'OOM'
    | 'JAVA_VERSION_MISMATCH'
    | 'LEGACY_FORGE_JAVA8'
    | 'MISSING_DEPENDENCY'
    | 'DUPLICATE_MOD'
    | 'MIXIN_FAILURE'
    | 'GPU_DRIVER';
  severity: 'critical' | 'warning';
  summary: string;
  culprit: string;
  excerpt: string;
  fixActionLabel: string;
  fixApplied: boolean;
}

export interface DiscoverItemRecord {
  id: string;
  title: string;
  slug: string;
  summary: string;
  author: string;
  provider: 'modrinth' | 'curseforge';
  category: 'Mod' | 'Modpack' | 'Resource Pack' | 'Shader';
  downloads: string;
  mcVersions: string[];
  loaders: string[];
  dependencies: string[];
  restrictedManualDownload?: boolean;
}

const TYPICAL_INSTANCES_SEED: Array<Omit<InstanceRecord, 'searchKey'>> = [
  {
    id: 'inst-26-3-fabric',
    name: 'Luminous 26.3 Fabric+',
    mcVersion: '26.3',
    loader: 'fabric',
    loaderVersion: '0.16.10',
    javaMajor: 25,
    playtimeMinutes: 4820,
    lastPlayedLabel: '14 mins ago',
    favorite: true,
    tags: ['modded', 'recent', 'favorites', 'year-26'],
    accentColor: '#10B981',
    modCount: 84,
    dataVersion: 4325,
    activeProfile: 'Performance',
    ramMb: 6144,
  },
  {
    id: 'inst-26-4-snap',
    name: '26.4 Snapshot Lab',
    mcVersion: '26.4-snapshot-4',
    loader: 'vanilla',
    loaderVersion: 'none',
    javaMajor: 25,
    playtimeMinutes: 310,
    lastPlayedLabel: 'Yesterday',
    favorite: true,
    tags: ['vanilla', 'snapshots', 'recent', 'favorites'],
    accentColor: '#38BDF8',
    modCount: 0,
    dataVersion: 4340,
    activeProfile: 'Default',
    ramMb: 4096,
  },
  {
    id: 'inst-1-21-4-neoforge',
    name: 'NeoTech Horizons 1.21.4',
    mcVersion: '1.21.4',
    loader: 'neoforge',
    loaderVersion: '21.4.88-beta',
    javaMajor: 21,
    playtimeMinutes: 2940,
    lastPlayedLabel: '2 days ago',
    favorite: true,
    tags: ['modded', 'recent', 'favorites'],
    accentColor: '#F59E0B',
    modCount: 162,
    dataVersion: 4189,
    activeProfile: 'Full',
    ramMb: 8192,
  },
  {
    id: 'inst-1-20-1-forge',
    name: 'Create: Astral Foundry',
    mcVersion: '1.20.1',
    loader: 'forge',
    loaderVersion: '47.3.12',
    javaMajor: 17,
    playtimeMinutes: 6190,
    lastPlayedLabel: '4 days ago',
    favorite: false,
    tags: ['modded'],
    accentColor: '#A855F7',
    modCount: 215,
    dataVersion: 3465,
    activeProfile: 'Default',
    ramMb: 8192,
  },
  {
    id: 'inst-1-16-5-quilt',
    name: 'Quilt Skyblock 1.16.5',
    mcVersion: '1.16.5',
    loader: 'quilt',
    loaderVersion: '0.26.3',
    javaMajor: 16,
    playtimeMinutes: 920,
    lastPlayedLabel: '1 week ago',
    favorite: false,
    tags: ['modded'],
    accentColor: '#EC4899',
    modCount: 48,
    dataVersion: 2586,
    activeProfile: 'Performance',
    ramMb: 4096,
  },
  {
    id: 'inst-1-12-2-forge',
    name: 'Enigmatica 2 Expert Legacy',
    mcVersion: '1.12.2',
    loader: 'forge',
    loaderVersion: '14.23.5.2860',
    javaMajor: 8,
    playtimeMinutes: 11420,
    lastPlayedLabel: '2 weeks ago',
    favorite: true,
    tags: ['modded', 'favorites', 'legacy'],
    accentColor: '#14B8A6',
    modCount: 248,
    dataVersion: 1343,
    activeProfile: 'Full',
    ramMb: 6144,
  },
  {
    id: 'inst-1-7-10-gtnh',
    name: 'GregTech New Horizons 1.7.10',
    mcVersion: '1.7.10',
    loader: 'forge',
    loaderVersion: '10.13.4.1614',
    javaMajor: 8,
    playtimeMinutes: 18400,
    lastPlayedLabel: '3 weeks ago',
    favorite: true,
    tags: ['modded', 'favorites', 'legacy'],
    accentColor: '#EF4444',
    modCount: 290,
    dataVersion: 171,
    activeProfile: 'Full',
    ramMb: 8192,
  },
  {
    id: 'inst-b1-7-3-retro',
    name: 'Golden Age Beta 1.7.3',
    mcVersion: 'b1.7.3',
    loader: 'vanilla',
    loaderVersion: 'none',
    javaMajor: 8,
    playtimeMinutes: 640,
    lastPlayedLabel: '1 month ago',
    favorite: false,
    tags: ['vanilla', 'legacy'],
    accentColor: '#84CC16',
    modCount: 0,
    dataVersion: 73,
    activeProfile: 'Default',
    ramMb: 2048,
  },
  {
    id: 'inst-a1-2-6-alpha',
    name: 'Alpha Monolith a1.2.6',
    mcVersion: 'a1.2.6',
    loader: 'vanilla',
    loaderVersion: 'none',
    javaMajor: 8,
    playtimeMinutes: 215,
    lastPlayedLabel: '2 months ago',
    favorite: false,
    tags: ['vanilla', 'legacy'],
    accentColor: '#22C55E',
    modCount: 0,
    dataVersion: 12,
    activeProfile: 'Default',
    ramMb: 2048,
  },
  {
    id: 'inst-26-3-vanilla',
    name: 'Pure Vanilla 26.3 SMP',
    mcVersion: '26.3',
    loader: 'vanilla',
    loaderVersion: 'none',
    javaMajor: 25,
    playtimeMinutes: 1280,
    lastPlayedLabel: '3 days ago',
    favorite: false,
    tags: ['vanilla', 'recent', 'year-26'],
    accentColor: '#10B981',
    modCount: 0,
    dataVersion: 4325,
    activeProfile: 'Default',
    ramMb: 4096,
  },
  {
    id: 'inst-1-21-1-fabric-shaders',
    name: 'Iris & Bliss Cinema 1.21.1',
    mcVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.9',
    javaMajor: 21,
    playtimeMinutes: 1750,
    lastPlayedLabel: '5 days ago',
    favorite: false,
    tags: ['modded'],
    accentColor: '#06B6D4',
    modCount: 64,
    dataVersion: 3955,
    activeProfile: 'Performance',
    ramMb: 6144,
  },
  {
    id: 'inst-c0-30-classic',
    name: 'Classic 0.30 Museum',
    mcVersion: 'c0.30_01c',
    loader: 'vanilla',
    loaderVersion: 'none',
    javaMajor: 8,
    playtimeMinutes: 90,
    lastPlayedLabel: '3 months ago',
    favorite: false,
    tags: ['vanilla', 'legacy'],
    accentColor: '#6366F1',
    modCount: 0,
    dataVersion: 1,
    activeProfile: 'Default',
    ramMb: 1024,
  },
];

export function buildInstancesForPreset(preset: FixturePreset): InstanceRecord[] {
  if (preset === 'empty') {
    return [];
  }
  const base = TYPICAL_INSTANCES_SEED.map((inst) => ({
    ...inst,
    searchKey: `${inst.name} ${inst.mcVersion} ${inst.loader} ${inst.tags.join(' ')}`.toLowerCase(),
  }));

  if (preset === 'typical') {
    return base;
  }

  // Stress preset: 1,000 instances
  const stressList: InstanceRecord[] = [...base];
  const versions = ['26.3', '26.4-snapshot-4', '1.21.4', '1.20.1', '1.16.5', '1.12.2', '1.7.10', 'b1.7.3'];
  const loaders: Array<InstanceRecord['loader']> = ['fabric', 'neoforge', 'forge', 'quilt', 'vanilla'];
  const accents = ['#10B981', '#38BDF8', '#F59E0B', '#A855F7', '#EC4899', '#14B8A6'];

  for (let i = stressList.length; i < 1000; i++) {
    const mcVersion = versions[i % versions.length];
    const loader = loaders[i % loaders.length];
    const name = `Stress Instance #${i.toString().padStart(4, '0')} (${mcVersion})`;
    const tags = [
      loader === 'vanilla' ? 'vanilla' : 'modded',
      mcVersion.includes('snapshot') ? 'snapshots' : 'release',
      ...(i % 7 === 0 ? ['favorites'] : []),
      ...(i < 35 ? ['recent'] : []),
    ];
    stressList.push({
      id: `inst-stress-${i.toString().padStart(4, '0')}`,
      name,
      mcVersion,
      loader,
      loaderVersion: loader === 'vanilla' ? 'none' : '0.16.10',
      javaMajor: mcVersion.startsWith('26.') ? 25 : mcVersion.startsWith('1.21') ? 21 : 17,
      playtimeMinutes: (i * 29) % 9500,
      lastPlayedLabel: `${(i % 28) + 1}d ago`,
      favorite: i % 7 === 0,
      tags,
      accentColor: accents[i % accents.length],
      modCount: loader === 'vanilla' ? 0 : 40 + (i % 180),
      dataVersion: 4325,
      activeProfile: i % 3 === 0 ? 'Performance' : i % 3 === 1 ? 'Full' : 'Default',
      ramMb: 4096,
      searchKey: `${name} ${mcVersion} ${loader} ${tags.join(' ')}`.toLowerCase(),
    });
  }
  return stressList;
}

const CORE_MOD_NAMES = [
  ['Sodium', 'sodium', 'modrinth'],
  ['Lithium', 'lithium', 'modrinth'],
  ['FerriteCore', 'ferrite-core', 'modrinth'],
  ['Iris Shaders', 'iris', 'modrinth'],
  ['Mod Menu', 'modmenu', 'modrinth'],
  ['Fabric API', 'fabric-api', 'modrinth'],
  ['EntityCulling', 'entityculling', 'curseforge'],
  ['ImmediatelyFast', 'immediatelyfast', 'modrinth'],
  ['ModernFix', 'modernfix', 'curseforge'],
  ['Distant Horizons', 'distant-horizons', 'modrinth'],
  ['REI (Roughly Enough Items)', 'rei', 'curseforge'],
  ['Jade (WTHIT)', 'jade', 'modrinth'],
  ['AppleSkin', 'appleskin', 'modrinth'],
  ['Sound Physics Remastered', 'sound-physics', 'modrinth'],
  ['Custom Local Optimization Jar', 'local-tweak', 'local'],
] as const;

export function buildModsForPreset(preset: FixturePreset): ModRecord[] {
  const count = preset === 'stress' ? 2000 : 60;
  const list: ModRecord[] = [];
  for (let i = 0; i < count; i++) {
    const seed = CORE_MOD_NAMES[i % CORE_MOD_NAMES.length];
    const suffix = i < CORE_MOD_NAMES.length ? '' : ` Module #${i}`;
    const name = `${seed[0]}${suffix}`;
    const slug = `${seed[1]}-${i}`;
    const provider = seed[2];
    const hasUpdate = i % 9 === 0;
    const enabled = i % 11 !== 0;
    const profiles: Array<'Default' | 'Performance' | 'Full'> =
      i % 4 === 0
        ? ['Default', 'Performance', 'Full']
        : i % 2 === 0
          ? ['Performance', 'Full']
          : ['Default', 'Full'];

    list.push({
      id: `mod-${i.toString().padStart(4, '0')}`,
      slug,
      name,
      version: `0.${(i % 9) + 2}.${i % 20}+mc26.3`,
      latestVersion: hasUpdate ? `0.${(i % 9) + 3}.0+mc26.3` : `0.${(i % 9) + 2}.${i % 20}+mc26.3`,
      hasUpdate,
      provider,
      enabled,
      sizeKb: 140 + ((i * 53) % 2400),
      environment: i % 5 === 0 ? 'client' : 'client+server',
      profiles,
      searchKey: `${name} ${slug} ${provider}`.toLowerCase(),
    });
  }
  return list;
}

export const TYPICAL_WORLDS: WorldRecord[] = [
  {
    id: 'world-monolith-26-3',
    name: 'Monolith Sanctuary (26.3)',
    folderName: 'Monolith_Sanctuary_26_3',
    mcVersionName: '26.3',
    dataVersion: 4325,
    gameMode: 'Survival',
    difficulty: 'Hard',
    cheats: false,
    lastPlayed: 'Today, 18:40',
    sizeMb: 412.8,
    pinnedToHome: true,
  },
  {
    id: 'world-redstone-lab',
    name: 'Compute Shader & Redstone Lab',
    folderName: 'Redstone_Lab_26_4',
    mcVersionName: '26.4-snapshot-4',
    dataVersion: 4340,
    gameMode: 'Creative',
    difficulty: 'Peaceful',
    cheats: true,
    lastPlayed: 'Yesterday',
    sizeMb: 86.4,
    pinnedToHome: true,
  },
  {
    id: 'world-hardcore-sky',
    name: 'Hardcore Archipelago #14',
    folderName: 'Hardcore_Archipelago_14',
    mcVersionName: '1.21.4',
    dataVersion: 4189,
    gameMode: 'Hardcore',
    difficulty: 'Hard',
    cheats: false,
    lastPlayed: '4 days ago',
    sizeMb: 628.1,
    pinnedToHome: false,
  },
];

export const TYPICAL_SERVERS: ServerRecord[] = [
  {
    id: 'srv-kestryx-smp',
    name: 'Kestryx Patrons 26.3 SMP',
    address: 'play.kestryx.gg:25565',
    motd: '§aLuminous Obsidian Season 4 §7• §bJava 25 + Fabric 26.3',
    playersOnline: 42,
    playersMax: 100,
    pingMs: 18,
    versionTag: '26.3',
    pinnedToHome: true,
  },
  {
    id: 'srv-hypixel',
    name: 'Hypixel Network',
    address: 'mc.hypixel.net',
    motd: '§eHypixel Network §c[1.8-26.x] §bSkyBlock Dungeons',
    playersOnline: 46820,
    playersMax: 200000,
    pingMs: 34,
    versionTag: '1.8.9 - 26.3',
    pinnedToHome: true,
  },
  {
    id: 'srv-local-lan',
    name: 'Studio Testing Node (LAN)',
    address: '127.0.0.1:25565',
    motd: 'Local Fabric Profiler Test Server',
    playersOnline: 2,
    playersMax: 20,
    pingMs: 1,
    versionTag: '26.3',
    pinnedToHome: false,
  },
];

export function buildDefaultAccounts(): AccountRecord[] {
  const notch = computeOfflinePlayerUuid('Notch');
  const steve = computeOfflinePlayerUuid('Steve');
  return [
    {
      id: 'acc-msa-main',
      username: 'AriaValkyrie',
      type: 'microsoft',
      uuidDashed: '7c9e6679-7425-40de-944b-e07fc1f90ae7',
      uuidUndashed: '7c9e6679742540de944be07fc1f90ae7',
      active: true,
      skinVariant: 'slim',
      capeId: 'migrator_emerald',
      tokenStatus: 'valid',
    },
    {
      id: 'acc-offline-notch',
      username: 'Notch',
      type: 'offline',
      uuidDashed: notch.dashed,
      uuidUndashed: notch.undashed,
      active: false,
      skinVariant: 'classic',
      capeId: 'obsidian_guard',
      tokenStatus: 'offline_local',
    },
    {
      id: 'acc-offline-steve',
      username: 'Steve',
      type: 'offline',
      uuidDashed: steve.dashed,
      uuidUndashed: steve.undashed,
      active: false,
      skinVariant: 'classic',
      capeId: 'none',
      tokenStatus: 'offline_local',
    },
  ];
}

export const TYPICAL_JAVA_RUNTIMES: JavaRuntimeRecord[] = [
  {
    id: 'java-25-temurin',
    major: 25,
    vendor: 'Eclipse Temurin (Adoptium)',
    versionString: '25.0.1+9-LTS',
    path: 'C:/Users/Player/AppData/Roaming/Kestryx/runtimes/temurin-25/bin/javaw.exe',
    arch: 'x86_64',
    managed: true,
    supportsAotCache: true,
    supportsAppCds: true,
    status: 'Verified',
  },
  {
    id: 'java-21-temurin',
    major: 21,
    vendor: 'Eclipse Temurin (Adoptium)',
    versionString: '21.0.6+7-LTS',
    path: 'C:/Users/Player/AppData/Roaming/Kestryx/runtimes/temurin-21/bin/javaw.exe',
    arch: 'x86_64',
    managed: true,
    supportsAotCache: false,
    supportsAppCds: true,
    status: 'Verified',
  },
  {
    id: 'java-17-zulu',
    major: 17,
    vendor: 'Azul Zulu',
    versionString: '17.0.14+7-LTS',
    path: 'C:/Program Files/Zulu/zulu-17/bin/javaw.exe',
    arch: 'x86_64',
    managed: false,
    supportsAotCache: false,
    supportsAppCds: true,
    status: 'Verified',
  },
  {
    id: 'java-16-temurin',
    major: 16,
    vendor: 'AdoptOpenJDK',
    versionString: '16.0.2+7',
    path: 'C:/Users/Player/AppData/Roaming/Kestryx/runtimes/temurin-16/bin/javaw.exe',
    arch: 'x86_64',
    managed: true,
    supportsAotCache: false,
    supportsAppCds: true,
    status: 'Ready',
  },
  {
    id: 'java-8-temurin',
    major: 8,
    vendor: 'Eclipse Temurin Legacy',
    versionString: '1.8.0_442-b06',
    path: 'C:/Users/Player/AppData/Roaming/Kestryx/runtimes/temurin-8/bin/javaw.exe',
    arch: 'x86_64',
    managed: true,
    supportsAotCache: false,
    supportsAppCds: false,
    status: 'Verified',
  },
];

export const TYPICAL_CRASH_SIGNATURES: CrashSignatureRecord[] = [
  {
    id: 'crash-java-ver',
    code: 'UNSUPPORTED_CLASS_VERSION_69',
    title: 'UnsupportedClassVersionError (Class file version 69.0 requires Java 25)',
    category: 'JAVA_VERSION_MISMATCH',
    severity: 'critical',
    summary:
      'Minecraft 26.3 class files were compiled for Java 25 (class version 69.0), but the instance attempted to boot with Java 21 (class version 65.0).',
    culprit: 'Runtime JVM mismatch (Java 21 selected instead of Java 25)',
    excerpt:
      'java.lang.UnsupportedClassVersionError: net/minecraft/client/main/Main has been compiled by a more recent version of the Java Runtime (class file version 69.0)',
    fixActionLabel: 'Auto-Switch Instance to Managed Java 25.0.1',
    fixApplied: false,
  },
  {
    id: 'crash-oom',
    code: 'JAVA_HEAP_OOM',
    title: 'java.lang.OutOfMemoryError: Java heap space during texture atlas stitch',
    category: 'OOM',
    severity: 'critical',
    summary:
      'Heap exhausted at 4,096 MB while stitching 215 mods + 128x resource pack. System has 32 GB physical RAM available.',
    culprit: 'Insufficient -Xmx heap allocation (4096M)',
    excerpt:
      'java.lang.OutOfMemoryError: Java heap space at net.minecraft.client.renderer.texture.Stitcher.stitch(Stitcher.java:94)',
    fixActionLabel: 'Raise Instance Allocation to Recommended 6,144 MB',
    fixApplied: false,
  },
  {
    id: 'crash-mixin',
    code: 'MIXIN_APPLY_ERROR',
    title: 'MixinApplyError: sodium-extra.mixins.json:MixinWorldRenderer failed injection',
    category: 'MIXIN_FAILURE',
    severity: 'critical',
    summary:
      'Incompatible version of Sodium Extra (built for Sodium 0.5) injected into Sodium 0.6.4 on Minecraft 26.3.',
    culprit: 'mod: sodium-extra-0.5.4+mc1.21.jar',
    excerpt:
      'org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered at sodium-extra.mixins.json:MixinWorldRenderer',
    fixActionLabel: 'Quarantine Conflicting Mod & Update to 26.3 Build',
    fixApplied: false,
  },
  {
    id: 'crash-legacy-forge',
    code: 'LEGACY_FORGE_JAVA8_CME',
    title: 'Legacy Forge 1.7.10 / 1.12.2 LaunchWrapper ConcurrentModificationException',
    category: 'LEGACY_FORGE_JAVA8',
    severity: 'warning',
    summary:
      'Legacy Forge FML sort failed due to modern Java Update version parsing or missing LegacyJavaFixer.',
    culprit: 'FML CoreModManager on 1.12.2',
    excerpt:
      'java.util.ConcurrentModificationException at java.util.ArrayList$Itr.checkForComodification(ArrayList.java:911) at net.minecraftforge.fml.relauncher.CoreModManager',
    fixActionLabel: 'Pin Java 8u442 + Inject LegacyJavaFixer Flag',
    fixApplied: false,
  },
];

export const DISCOVER_CATALOG: DiscoverItemRecord[] = [
  {
    id: 'disc-sodium',
    title: 'Sodium',
    slug: 'sodium',
    summary: 'Modern rendering engine for Minecraft that drastically improves frame rates and reduces micro-stutter.',
    author: 'jellysquid3',
    provider: 'modrinth',
    category: 'Mod',
    downloads: '54.2M',
    mcVersions: ['26.3', '26.4-snapshot-4', '1.21.4', '1.20.1'],
    loaders: ['fabric', 'neoforge', 'quilt'],
    dependencies: ['fabric-api'],
  },
  {
    id: 'disc-iris',
    title: 'Iris Shaders',
    slug: 'iris',
    summary: 'Modern shader pack loader for Minecraft paired with Sodium for high-FPS PBR & raytraced shaders.',
    author: 'coderbot',
    provider: 'modrinth',
    category: 'Mod',
    downloads: '39.8M',
    mcVersions: ['26.3', '1.21.4', '1.20.1'],
    loaders: ['fabric', 'neoforge', 'quilt'],
    dependencies: ['sodium', 'fabric-api'],
  },
  {
    id: 'disc-create-cf',
    title: 'Create: Clockwork & Steam (Distribution Restricted Demo)',
    slug: 'create-clockwork-restricted',
    summary: 'Contraptions, kinetic steampunk factories, and trains. Demonstrates CurseForge third-party opt-out flow.',
    author: 'simibubi_addon',
    provider: 'curseforge',
    category: 'Mod',
    downloads: '18.4M',
    mcVersions: ['26.3', '1.21.4', '1.20.1'],
    loaders: ['neoforge', 'forge', 'fabric'],
    dependencies: ['flywheel-lib'],
    restrictedManualDownload: true,
  },
  {
    id: 'disc-complementary',
    title: 'Complementary Reimagined Shaders',
    slug: 'complementary-reimagined',
    summary: 'Preserves the iconic look of Minecraft while adding volumetric lighting, water reflections, and crisp shadows.',
    author: 'EminGT',
    provider: 'modrinth',
    category: 'Shader',
    downloads: '27.1M',
    mcVersions: ['26.3', '1.21.4', '1.20.1', '1.12.2'],
    loaders: ['fabric', 'neoforge', 'forge', 'quilt'],
    dependencies: ['iris'],
  },
  {
    id: 'disc-luminous-pack',
    title: 'Luminous 26.3 Optimization Pack (.mrpack)',
    slug: 'luminous-26-3-mrpack',
    summary: 'Pre-configured 26.3 Fabric performance suite with Sodium, Lithium, FerriteCore, and ImmediatelyFast.',
    author: 'KestryxTeam',
    provider: 'modrinth',
    category: 'Modpack',
    downloads: '1.9M',
    mcVersions: ['26.3'],
    loaders: ['fabric'],
    dependencies: ['sodium', 'lithium', 'ferrite-core', 'fabric-api'],
  },
];
