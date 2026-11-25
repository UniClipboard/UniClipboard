export type IosSettingsPage =
  | 'root'
  | 'sync'
  | 'history'
  | 'storage'
  | 'network'
  | 'clipboard'
  | 'share'
  | 'diagnostics'
  | 'privacy'
  | 'about';

export type IosSettingFieldEdit = {
  id: string;
  title: string;
  value: string;
  placeholder?: string;
  save: (value: string) => Promise<void>;
};
