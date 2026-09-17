import { AlertTriangle, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { getMaintenance, setMaintenance } from "@/api";
import { useApi } from "@/hooks/useApi";
import { useEvents } from "@/hooks/useEvents";
import { toast } from "@/hooks/useToast";
import { useState } from "react";
import { t } from "@/lib/i18n";

/** 维护模式横幅:激活时显示在 header 下方,可一键恢复调度。 */
export function MaintenanceBanner() {
  const { data, reload } = useApi(getMaintenance, []);
  useEvents(reload);
  const [busy, setBusy] = useState(false);

  if (!data?.enabled) return null;

  return (
    <div className="flex items-center gap-2 border-b border-amber-500/30 bg-amber-500/10 px-4 py-2 text-sm text-amber-600 dark:text-amber-400">
      <AlertTriangle className="h-4 w-4 shrink-0" />
      <span className="text-xs font-medium">{t("Maintenance mode is on — scheduled runs are paused.")}</span>
      <div className="flex-1" />
      <Button
        size="sm"
        variant="outline"
        disabled={busy}
        className="h-7 text-xs border-amber-500/40 text-amber-600 dark:text-amber-400"
        onClick={async () => {
          setBusy(true);
          try {
            await setMaintenance(false);
            toast.success(t("Maintenance mode disabled"));
            await reload();
          } catch (e) {
            toast.error(e instanceof Error ? e.message : String(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
        {t("Resume scheduling")}
      </Button>
    </div>
  );
}
