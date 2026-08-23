import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";

/**
 * A single persisted app setting, backed by the `application_settings`
 * table (not localStorage) so it survives reinstalls/moves the same way
 * everything else ModForge remembers does.
 */
export function useAppSetting(key: string, defaultValue: string) {
  const [value, setValue] = useState(defaultValue);
  const [isLoaded, setIsLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .getAppSetting(key)
      .then((stored) => {
        if (!cancelled) {
          setValue(stored ?? defaultValue);
          setIsLoaded(true);
        }
      })
      .catch(() => setIsLoaded(true));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  async function update(next: string) {
    setValue(next);
    await api.setAppSetting(key, next);
  }

  return { value, setValue: update, isLoaded };
}

export function useBoolAppSetting(key: string, defaultValue: boolean) {
  const { value, setValue, isLoaded } = useAppSetting(key, String(defaultValue));
  return {
    value: value === "true",
    setValue: (next: boolean) => setValue(String(next)),
    isLoaded,
  };
}
