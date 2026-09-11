export type InstalledApp = {
  name: string;
  version: string;
  publisher: string;
  install_location: string;
  uninstall_string: string;
  quiet_uninstall_string: string;
  source: string;
  registry_key: string;
  estimated_size_kb: number;
};
