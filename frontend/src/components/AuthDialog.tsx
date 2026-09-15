import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { getToken, setToken } from "@/api";

/** 服务端启用 SCHEDULE_TOKEN 后,任意请求 401 会触发本对话框收集 token。 */
export function AuthDialog() {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");

  useEffect(() => {
    const handler = () => {
      setValue(getToken() ?? "");
      setOpen(true);
    };
    window.addEventListener("auth-required", handler);
    return () => window.removeEventListener("auth-required", handler);
  }, []);

  const save = () => {
    setToken(value.trim());
    setOpen(false);
    window.location.reload();
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-sm" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>Authentication required</DialogTitle>
          <DialogDescription>
            This server requires an API token. Paste your token to continue.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="auth-token">API Token</Label>
          <Input
            id="auth-token"
            type="password"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder="SCHEDULE_TOKEN"
            onKeyDown={(e) => e.key === "Enter" && value.trim() && save()}
            className="font-mono"
          />
        </div>
        <DialogFooter>
          <Button onClick={save} disabled={!value.trim()}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
