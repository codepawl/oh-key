declare namespace Cloudflare {
  interface Env {
    REPORTS: KVNamespace;
    GITHUB_TOKEN: string;
    GITHUB_REPO: string;
    MAX_REPORTS_PER_IP_PER_HOUR: string;
    MAX_NEW_ISSUES_PER_DAY: string;
    MAX_NEW_LOW_PRIORITY_ISSUES_PER_DAY: string;
  }
}

interface Env extends Cloudflare.Env {}
