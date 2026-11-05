/**
 * The public demo page (`npm run build:demo`, Vite mode `demo`): the Console on
 * the simulated desk in the browser. It has no dagon serve behind it, so it
 * reads and writes no files, opens no device and holds no keys; the controls
 * for those stay visible and say why on hover or press. Every other build
 * leaves this false and the code it guards out of the bundle.
 */
export const DEMO_SITE: boolean = import.meta.env.MODE === 'demo';
