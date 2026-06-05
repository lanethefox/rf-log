import { create } from 'zustand'

export const MISSIONS = ['dashboard', 'survey', 'hunt', 'drone', 'tscm', 'report']

export const useAppStore = create((set) => ({
  mission: 'dashboard',
  setMission: (mission) => set({ mission }),
}))
