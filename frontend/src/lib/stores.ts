import { writable } from 'svelte/store'
import type { Vehicle, RemindersResponse, GarageDashboard } from './types'
import { dashboard as dashboardApi } from './api'

export const selectedVehicle = writable<Vehicle | null>(null)
export const vehicleReminders = writable<RemindersResponse | null>(null)

/** One shared `/api/dashboard` snapshot: the sidebar's status hints and the
 *  garage dashboard both read it; mutations call `refreshDashboard()`.
 *  Concurrent callers share one in-flight request. */
export const garageDashboard = writable<GarageDashboard | null>(null)

let inflight: Promise<GarageDashboard> | null = null

export async function refreshDashboard(): Promise<GarageDashboard> {
  if (!inflight) {
    inflight = dashboardApi
      .get()
      .then((d) => {
        garageDashboard.set(d)
        return d
      })
      .finally(() => {
        inflight = null
      })
  }
  return inflight
}
