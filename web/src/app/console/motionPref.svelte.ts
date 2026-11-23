// Looping motion paused from the menu (WCAG 2.2.2), kept across reloads; base.css pauses it under `data-still`.

const KEY = 'dagon-motion-still';

export class MotionPref {
  still = $state(MotionPref.read());

  /** Call it where an effect may be made (a component's script). */
  constructor() {
    $effect(() => {
      document.documentElement.toggleAttribute('data-still', this.still);
      try {
        localStorage.setItem(KEY, this.still ? '1' : '0');
      } catch {
        // Storage may be off: the choice then lasts for this page only.
      }
    });
  }

  private static read(): boolean {
    try {
      return localStorage.getItem(KEY) === '1';
    } catch {
      return false;
    }
  }
}
