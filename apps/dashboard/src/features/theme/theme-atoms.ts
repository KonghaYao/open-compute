import { atom, useAtomValue, useSetAtom } from "jotai";

export type ThemeMode = "light" | "dark" | "system";

export const themeModeAtom = atom<ThemeMode>("system");
export const systemThemeAtom = atom<"light" | "dark">("light");
export const resolvedThemeAtom = atom<"light" | "dark">((get) => {
  const mode = get(themeModeAtom);
  return mode === "system" ? get(systemThemeAtom) : mode;
});

const setThemeModeAtom = atom(null, (_get, set, mode: ThemeMode) => {
  set(themeModeAtom, mode);
});

const toggleThemeAtom = atom(null, (get, set) => {
  set(setThemeModeAtom, get(resolvedThemeAtom) === "dark" ? "light" : "dark");
});

export function useTheme() {
  return {
    mode: useAtomValue(themeModeAtom),
    resolved: useAtomValue(resolvedThemeAtom),
    setMode: useSetAtom(setThemeModeAtom),
    toggle: useSetAtom(toggleThemeAtom),
  };
}
