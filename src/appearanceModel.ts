export type AppTheme = 'light' | 'dark' | 'system'
export type DisplayTheme = 'light' | 'dark'
export type Appearance = { appTheme: AppTheme; displayTheme: DisplayTheme; revision: number }
export const defaultAppearance: Appearance = { appTheme: 'system', displayTheme: 'dark', revision: 0 }
export const resolveAppTheme = (theme: AppTheme, systemDark: boolean): DisplayTheme =>
  theme === 'system' ? systemDark ? 'dark' : 'light' : theme
export const latestAppearance = (current: Appearance, next: Appearance) => next.revision >= current.revision ? next : current
