import { defineCollection } from 'astro:content';
import { docsLoader, i18nLoader } from '@astrojs/starlight/loaders';
import { docsSchema, i18nSchema } from '@astrojs/starlight/schema';

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  // A Japanese page that falls back to English says only that it is in English.
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
