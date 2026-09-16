import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  listApiTokens,
  createApiToken,
  revokeApiToken,
  listAudit,
  type ApiTokenInfo,
  type AuditEntry,
} from "@/api";
import { toast } from "@/hooks/useToast";
import { timeAgo } from "@/lib/time";
import { t } from "@/lib/i18n";
import { Plus, Trash2 } from "lucide-react";

/** 设置对话框:API Token 管理 + 审计日志。 */
export function SettingsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [tokens, setTokens] = useState<ApiTokenInfo[]>([]);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [newName, setNewName] = useState("");
  const [minted, setMinted] = useState<string | null>(null);

  const load = async () => {
    try {
      setTokens(await listApiTokens());
      setAudit(await listAudit(50));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    if (open) load();
  }, [open]);

  const create = async () => {
    if (!newName.trim()) return;
    try {
      const r = await createApiToken(newName.trim());
      setMinted(r.token);
      setNewName("");
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  const revoke = async (id: string) => {
    try {
      await revokeApiToken(id);
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("Settings")}</DialogTitle>
          <DialogDescription>{t("API tokens and audit trail.")}</DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="tokens">
          <TabsList className="w-full">
            <TabsTrigger value="tokens" className="flex-1">{t("API Tokens")}</TabsTrigger>
            <TabsTrigger value="audit" className="flex-1">{t("Audit Log")}</TabsTrigger>
          </TabsList>

          <div className="mt-3 space-y-3">
            <div className="space-y-2">
              <div className="flex gap-2">
                <Input
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  placeholder={t("Token name")}
                  className="flex-1"
                  onKeyDown={(e) => e.key === "Enter" && newName.trim() && create()}
                />
                <Button size="sm" onClick={create} disabled={!newName.trim()}>
                  <Plus className="h-4 w-4" /> {t("Create")}
                </Button>
              </div>
              {minted && (
                <div className="rounded-md border border-emerald-500/40 bg-emerald-500/10 p-2.5 text-xs space-y-1">
                  <p className="font-medium text-foreground">
                    {t("Token created — copy it now, it will not be shown again")}:
                  </p>
                  <code className="block break-all font-mono text-muted-foreground">{minted}</code>
                </div>
              )}
              <div className="space-y-1">
                {tokens.length === 0 ? (
                  <p className="text-xs text-muted-foreground py-2 text-center">{t("No tokens")}</p>
                ) : (
                  tokens.map((tok) => (
                    <div key={tok.id} className="flex items-center justify-between rounded-md border border-border/50 px-2.5 py-1.5">
                      <div className="min-w-0">
                        <p className="text-sm truncate">{tok.name}</p>
                        <p className="text-xs text-muted-foreground tabular-nums">{timeAgo(tok.created_at)}</p>
                      </div>
                      <Button size="icon-sm" variant="ghost" onClick={() => revoke(tok.id)} title={t("Revoke")}>
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    </div>
                  ))
                )}
              </div>
            </div>
          </div>

          <div className="mt-3">
            <div className="space-y-1 max-h-64 overflow-y-auto">
              {audit.length === 0 ? (
                <p className="text-xs text-muted-foreground py-4 text-center">{t("No audit entries")}</p>
              ) : (
                audit.map((a, i) => (
                  <div key={i} className="flex items-center justify-between text-xs border-b border-border/30 pb-1.5 mb-1.5 last:border-0">
                    <span className="font-medium">{t(a.action) }</span>
                    <span className="text-muted-foreground truncate max-w-[55%]">{a.summary}</span>
                    <span className="text-muted-foreground/70 tabular-nums">{timeAgo(a.ts)}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
