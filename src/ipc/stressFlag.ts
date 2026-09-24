/** `/?stress` runs the office stress test (see stress.ts) instead of the recorded demo. */
export function stressRequested(): boolean {
  try {
    return new URLSearchParams(window.location.search).has("stress");
  } catch {
    return false;
  }
}
