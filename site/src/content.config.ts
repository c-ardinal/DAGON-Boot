import { defineCollection } from 'astro:content';
import { docsLoader, i18nLoader } from '@astrojs/starlight/loaders';
import { docsSchema, i18nSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  // A page shown under /en/ from the root says only that it has one language version.
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
