import { writable } from 'svelte/store'
import type { Vehicle, RemindersResponse } from './types'

export const selectedVehicle = writable<Vehicle | null>(null)
export const vehicleReminders = writable<RemindersResponse | null>(null)
