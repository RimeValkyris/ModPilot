import { useEffect } from "react";
import { useInstancesStore } from "@/stores/instancesStore";

/**
 * Loads instances on first mount and exposes the shared instances state and
 * mutations. Safe to call from multiple components - the underlying state
 * is a single shared zustand store, not per-component state.
 */
export function useInstances() {
  const {
    instances,
    isLoading,
    error,
    fetchInstances,
    createInstance,
    importInstance,
    renameInstance,
    duplicateInstance,
    setInstanceJava,
    updateInstanceSettings,
    deleteInstance,
  } = useInstancesStore();

  useEffect(() => {
    fetchInstances();
  }, [fetchInstances]);

  return {
    instances,
    isLoading,
    error,
    refetch: fetchInstances,
    createInstance,
    importInstance,
    renameInstance,
    duplicateInstance,
    setInstanceJava,
    updateInstanceSettings,
    deleteInstance,
  };
}
