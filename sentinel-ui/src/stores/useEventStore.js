import { create } from 'zustand'

const MAX_EVENTS = 200

export const useEventStore = create((set) => ({
  events: [],   // newest first
  pushEvent: (evt) => set(s => ({
    events: [evt, ...s.events].slice(0, MAX_EVENTS)
  })),
  clearEvents: () => set({ events: [] }),
}))
