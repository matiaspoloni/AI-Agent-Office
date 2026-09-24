import { createContext, useContext } from "react";
import type { Backend } from "./backend";

export const BackendContext = createContext<Backend | null>(null);

export function useBackend(): Backend {
  const backend = useContext(BackendContext);
  if (!backend) throw new Error("Backend not initialized");
  return backend;
}
