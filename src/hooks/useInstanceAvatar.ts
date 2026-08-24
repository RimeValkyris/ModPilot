import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";

/**
 * Loads an instance's profile picture (data URI, or `null` if none is set).
 * Shared by every place the avatar is shown - `InstanceCard`,
 * `InstanceDetailPage`, and `ServerPropertiesForm`'s picker preview all had
 * this same fetch-with-cancellation-guard effect copy-pasted before this.
 */
export function useInstanceAvatar(instanceId: string | undefined) {
  const [avatar, setAvatar] = useState<string | null>(null);

  useEffect(() => {
    if (!instanceId) {
      setAvatar(null);
      return;
    }
    let cancelled = false;
    api
      .readInstanceAvatar(instanceId)
      .then((dataUri) => {
        if (!cancelled) setAvatar(dataUri);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [instanceId]);

  return [avatar, setAvatar] as const;
}
