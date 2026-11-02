<script lang="ts">
  import { versionText, type HelloResp } from './serial/hello.ts';

  let { hello, label }: { hello: HelloResp; label: string } = $props();

  const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');

  // Second-tier view: raw HELLO fields under their protocol names.
  const rows = $derived([
    ['proto_ver', String(hello.protoVer)],
    ['role', String(hello.role)],
    ['port_kind', String(hello.portKind)],
    ['mcu_id', `0x${hello.mcuId.toString(16).padStart(4, '0')}`],
    ['mode', String(hello.mode)],
    ['active_version', versionText(hello.activeVersion)],
    ['trial', String(hello.trial)],
    ['key_id', hex(hello.keyId)],
    ['max_payload', String(hello.maxPayload)],
  ]);
</script>

<details>
  <summary>{label}</summary>
  <dl translate="no">
    {#each rows as [k, v] (k)}
      <dt>{k}</dt>
      <dd>{v}</dd>
    {/each}
  </dl>
</details>

<style>
  details {
    width: 100%;
  }
  /* Own border keeps 3 px of room: 1 px line normally, 3 px paper on focus. */
  summary {
    padding: 2px calc(var(--s-3) + 2px);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    cursor: pointer;
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  summary:focus-visible {
    padding: 0 var(--s-3);
    border: 3px solid var(--c-paper);
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
  @media (hover: hover) and (pointer: fine) {
    summary:hover {
      background: var(--c-hover);
    }
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2) var(--s-4);
    margin: var(--s-2) 0 0;
    font: 500 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
