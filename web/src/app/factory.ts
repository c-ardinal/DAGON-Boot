import { FLASH } from './copy.ts';
import type { Job } from './define/cli.svelte.ts';

/**
 * Direct writing over USB: serve's write job asks before it writes. The user has
 * already pressed 書き込む in the modal that says what will be written and how,
 * so that press is the answer: yes, once, and only to a question of this job.
 * Returns whether it answered.
 */
export function answerConsent(job: Job | null, answer: (job: Job, text: string) => unknown, answered: Set<string>): boolean {
  if (!job || job.kind !== 'flash' || job.state !== 'question' || !job.question) return false;
  const key = `${job.id}:${job.question.id}`;
  if (answered.has(key)) return false;
  answered.add(key);
  void answer(job, FLASH.yes);
  return true;
}
