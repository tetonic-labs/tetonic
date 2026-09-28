/** Optional detail panels; the conversation remains the everyday workspace. */
export const workspaceViews = [
  { id: 'home', label: 'Your team' },
  { id: 'huddle', label: 'Activity' },
  { id: 'teams', label: 'Teams' },
  { id: 'inbox', label: 'Approvals' },
  { id: 'graph', label: 'Connections' },
  { id: 'castle', label: 'Workstation' },
] as const;

export type WorkspaceView = (typeof workspaceViews)[number]['id'];
