export type Mode = "light" | "dark";

export function initialMode(stored: string | null, osPrefersDark: boolean): Mode {
  if (stored === "light" || stored === "dark") return stored;
  return osPrefersDark ? "dark" : "light";
}

class Theme {
  mode = $state<Mode>("light");

  private apply(mode: Mode) {
    this.mode = mode;
    document.documentElement.classList.toggle("dark", mode === "dark");
  }

  load() {
    this.apply(
      initialMode(localStorage.getItem("theme"), matchMedia("(prefers-color-scheme: dark)").matches),
    );
  }

  toggle() {
    this.apply(this.mode === "dark" ? "light" : "dark");
    localStorage.setItem("theme", this.mode);
  }
}

export const theme = new Theme();

if (import.meta.vitest) {
  const { test, expect } = import.meta.vitest;

  test("a saved choice wins, otherwise the OS decides", () => {
    expect(initialMode("light", true)).toBe("light");
    expect(initialMode("dark", false)).toBe("dark");
    expect(initialMode(null, true)).toBe("dark");
    expect(initialMode("nonsense", false)).toBe("light");
  });
}
