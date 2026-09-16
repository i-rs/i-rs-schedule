import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { setToken, login } from "@/api";
import { t } from "@/lib/i18n";

/** 服务端启用认证后,任意请求 401 会弹出本对话框:账密登录或手动粘贴 token。 */
export function AuthDialog() {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<"login" | "token">("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [tokenValue, setTokenValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const handler = () => {
      setError(null);
      setOpen(true);
    };
    window.addEventListener("auth-required", handler);
    return () => window.removeEventListener("auth-required", handler);
  }, []);

  const finish = (token: string) => {
    setToken(token);
    setOpen(false);
    window.location.reload();
  };

  const doLogin = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await login(username, password);
      finish(r.token);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-sm" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>{t("Authentication required")}</DialogTitle>
          <DialogDescription>
            {t("Sign in to continue, or paste an existing token.")}
          </DialogDescription>
        </DialogHeader>

        <Tabs value={mode} onValueChange={(v) => setMode(v as "login" | "token")}>
          <TabsList className="w-full">
            <TabsTrigger value="login" className="flex-1">{t("Sign in")}</TabsTrigger>
            <TabsTrigger value="token" className="flex-1">{t("Token")}</TabsTrigger>
          </TabsList>
        </Tabs>

        {mode === "login" ? (
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="auth-user">{t("Username")}</Label>
              <Input
                id="auth-user"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                autoComplete="username"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="auth-pass">{t("Password")}</Label>
              <Input
                id="auth-pass"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && username && password && doLogin()}
                autoComplete="current-password"
              />
            </div>
            {error && <p className="text-xs text-destructive">{error}</p>}
            <DialogFooter>
              <Button onClick={doLogin} disabled={!username || !password || busy}>
                {busy ? t("Signing in...") : t("Sign in")}
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="auth-token">{t("API Token")}</Label>
              <Input
                id="auth-token"
                type="password"
                value={tokenValue}
                onChange={(e) => setTokenValue(e.target.value)}
                placeholder="SCHEDULE_TOKEN"
                onKeyDown={(e) => e.key === "Enter" && tokenValue.trim() && finish(tokenValue.trim())}
                className="font-mono"
              />
            </div>
            <DialogFooter>
              <Button onClick={() => finish(tokenValue.trim())} disabled={!tokenValue.trim()}>
                {t("Save")}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
