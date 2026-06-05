import { create } from 'zustand'

export const useSpectrumStore = create((set) => ({
  frame: null,         // latest SpectrumFrame from WebSocket
  connected: false,
  setFrame: (frame) => set({ frame }),
  setConnected: (connected) => set({ connected }),
}))
