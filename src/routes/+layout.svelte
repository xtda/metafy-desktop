<script lang="ts">
  import type { Snippet } from "svelte";

  type NavItem = {
    label: string;
    status: string;
    href: string;
  };

  let { children }: { children: Snippet } = $props();

  const navItems: NavItem[] = [
    { label: "Record", status: "Ready", href: "/#record" },
    { label: "Library", status: "Ready", href: "/#library" },
    { label: "Search", status: "Stub", href: "/#search" },
    { label: "Settings", status: "Ready", href: "/#settings" },
  ];
</script>

<svelte:head>
  <title>Metafy Desktop</title>
  <meta
    name="description"
    content="Local-only desktop recording and knowledge assistant."
  />
</svelte:head>

<div class="app-shell">
  <aside class="sidebar" aria-label="Primary navigation">
    <a class="brand" href="/">
      <span class="brand-mark" aria-hidden="true">M</span>
      <span>
        <strong>Metafy Desktop</strong>
        <small>Local MVP</small>
      </span>
    </a>

    <nav class="nav-list" aria-label="Build areas">
      {#each navItems as item (item.label)}
        <a class="nav-item" href={item.href}>
          <span>{item.label}</span>
          <small>{item.status}</small>
        </a>
      {/each}
    </nav>

    <div class="local-status">
      <span class="status-dot" aria-hidden="true"></span>
      <span>Core workflow runs locally</span>
    </div>
  </aside>

  <main class="workspace">
    {@render children()}
  </main>
</div>

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(html) {
    min-width: 320px;
    background: #f4f6f1;
    color: #171914;
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
      sans-serif;
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  :global(body) {
    min-height: 100vh;
    margin: 0;
  }

  :global(button),
  :global(input),
  :global(select),
  :global(textarea) {
    font: inherit;
  }

  :global(button) {
    letter-spacing: 0;
  }

  :global(a) {
    color: inherit;
  }

  .app-shell {
    display: grid;
    min-height: 100vh;
    grid-template-columns: 280px minmax(0, 1fr);
  }

  .sidebar {
    display: flex;
    min-height: 100vh;
    flex-direction: column;
    gap: 28px;
    border-right: 1px solid #d9e1d5;
    background: #fbfcf8;
    padding: 24px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    color: #171914;
    text-decoration: none;
  }

  .brand-mark {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border-radius: 8px;
    background: #171914;
    color: #fbfcf8;
    font-weight: 760;
  }

  .brand strong,
  .brand small {
    display: block;
  }

  .brand strong {
    font-size: 0.98rem;
    line-height: 1.2;
  }

  .brand small {
    margin-top: 2px;
    color: #697065;
    font-size: 0.78rem;
  }

  .nav-list {
    display: grid;
    gap: 6px;
  }

  .nav-item {
    display: flex;
    min-height: 44px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    border-radius: 8px;
    padding: 10px 12px;
    color: #3a3f35;
    text-decoration: none;
  }

  .nav-item span {
    font-size: 0.92rem;
    font-weight: 650;
  }

  .nav-item small {
    color: #697065;
    font-size: 0.72rem;
  }

  .nav-item:hover {
    background: #e3ebe0;
    color: #162117;
  }

  .local-status {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: auto;
    border-top: 1px solid #dfe5da;
    padding-top: 18px;
    color: #3f493f;
    font-size: 0.84rem;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: #257858;
  }

  .workspace {
    min-width: 0;
    padding: 38px;
  }

  @media (max-width: 820px) {
    .app-shell {
      grid-template-columns: 1fr;
    }

    .sidebar {
      min-height: auto;
      border-right: 0;
      border-bottom: 1px solid #d9e1d5;
      padding: 18px;
    }

    .nav-list {
      display: flex;
      overflow-x: auto;
      padding-bottom: 2px;
    }

    .nav-item {
      min-width: 138px;
    }

    .local-status {
      margin-top: 0;
    }

    .workspace {
      padding: 24px 18px;
    }
  }
</style>
