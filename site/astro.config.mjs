// The DAGON documentation site, published at <site>/doc beside the Console demo at <site>/demo.
// SITE_URL (the Pages address, e.g. https://<owner>.github.io/<repo>) comes from the Pages workflow;
// without it the site builds for http://127.0.0.1:4321 (local preview).
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';

const url = new URL(process.env.SITE_URL ?? 'http://127.0.0.1:4321');
const root = url.pathname.replace(/\/$/, '');
const base = `${root}/doc`;

/**
 * Content links are written from the docs root (`/reference/boards/`), and `~/demo/` names the Console demo
 * beside the docs; the published address adds where the site lives.
 */
function linksFromBase() {
  const walk = (node) => {
    if (node.type === 'link' && node.url.startsWith('~/')) node.url = `${root}/${node.url.slice(2)}`;
    else if (node.type === 'link' && node.url.startsWith('/') && !node.url.startsWith(`${base}/`)) node.url = `${base}${node.url}`;
    node.children?.forEach(walk);
  };
  return walk;
}

export default defineConfig({
  site: url.origin,
  base,
  trailingSlash: 'always',
  outDir: './dist/doc',
  markdown: { processor: unified({ remarkPlugins: [linksFromBase] }) },
  integrations: [
    starlight({
      title: 'DAGON',
      // English is the default: the generated references come from English sources and the Japanese pages fall back to them.
      defaultLocale: 'root',
      locales: {
        root: { label: 'English', lang: 'en' },
        ja: { label: '日本語', lang: 'ja' },
      },
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/c-ardinal/DAGON-Boot' }],
      sidebar: [
        { label: 'Getting started', translations: { ja: 'はじめに' }, items: [{ autogenerate: { directory: 'getting-started' } }] },
        { label: 'How-to guides', translations: { ja: 'やり方' }, items: [{ autogenerate: { directory: 'how-to' } }] },
        { label: 'Explanation', translations: { ja: 'しくみ' }, items: [{ autogenerate: { directory: 'explanation' } }] },
        { label: 'Reference', translations: { ja: 'リファレンス' }, items: [{ autogenerate: { directory: 'reference' } }] },
      ],
      plugins: [
        starlightLinksValidator({
          // Japanese pages may fall back to the English ones.
          errorOnFallbackPages: false,
          // The Rust API pages are rustdoc's, copied in beside the site after it is built.
          // The demo is the Console's own build, beside the docs.
          exclude: [`${base}/api/**`, `${root}/demo/`],
        }),
      ],
    }),
  ],
});
