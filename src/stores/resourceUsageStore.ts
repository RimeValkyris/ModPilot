import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { ResourceUsage } from "@/types/monitor";

interface ResourceUsageState {
  usageByInstanceId: Record<string, ResourceUsage>;
  poll: () => Promise<void>;
}

/**
 * One shared poll for every running instance's resource usage, instead of
 * each instance card independently invoking its own command on its own
 * timer. With several servers running at once, that was N IPC round trips
 * every tick for data the backend can just as easily return in one.
 */
export const useResourceUsageStore = create<ResourceUsageState>((set) => ({
  usageByInstanceId: {},

  poll: async () => {
    try {
      const usageByInstanceId = await api.getAllResourceUsage();
      set({ usageByInstanceId });
    } catch {
      // Transient errors aren't worth surfacing for a background poll -
      // the next tick will just try again.
    }
  },
}));
