import { cache } from "react";
import { api, type ControlLock, type DeviceDetails, type User } from "@/lib/api";

export const devicePath = (id: string) => `/api/v1/devices/${encodeURIComponent(id)}`;
export const uptime = (s: number) => `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;

// The Device tab layout and the active tab both need these: `cache` keeps it one API read per request.
export const getDevice = cache((id: string) => api<DeviceDetails>(devicePath(id)));
export const getLock = cache((id: string) => api<ControlLock>(`${devicePath(id)}/lock`));
export const getMe = cache(() => api<User>("/api/v1/me"));
