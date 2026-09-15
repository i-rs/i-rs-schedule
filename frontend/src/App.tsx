import { BrowserRouter, Routes, Route, NavLink, useLocation } from "react-router-dom";
import { lazy, Suspense, useEffect } from "react";
const Dashboard = lazy(() => import("@/pages/Dashboard"));
const Tasks = lazy(() => import("@/pages/Tasks"));
const Executions = lazy(() => import("@/pages/Executions"));
import { LayoutDashboard, ListTodo, ScrollText, Sun, Moon, RefreshCw } from "lucide-react";
import { useTheme } from "@/hooks/useTheme";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SidebarProvider, Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuItem, SidebarTrigger, useSidebar } from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { TooltipProvider, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ToastContainer } from "@/components/ui/toast";
import { cn } from "@/lib/utils";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/executions", label: "Executions", icon: ScrollText },
];

const pageTitles: Record<string, string> = {
  "/": "Dashboard",
  "/tasks": "Tasks",
  "/executions": "Executions",
};

/** 按路由更新浏览器标签页标题。 */
function DocumentTitle() {
  const { pathname } = useLocation();
  useEffect(() => {
    document.title = `${pageTitles[pathname] ?? "i-rs-schedule"} · i-rs-schedule`;
  }, [pathname]);
  return null;
}

/** 顶栏中的当前页面名,给空旷的 header 提供上下文。 */
function HeaderPageName() {
  const { pathname } = useLocation();
  const name = pageTitles[pathname];
  if (!name) return null;
  return <span className="text-sm text-muted-foreground">{name}</span>;
}

function SidebarNav() {
  const { setOpenMobile, state } = useSidebar();
  const collapsed = state === "collapsed";

  const handleClick = () => setOpenMobile(false);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className={cn("flex items-center gap-2 py-2", collapsed ? "justify-center px-0" : "px-3")}>
          <div className="flex h-7 w-7 items-center justify-center rounded-md bg-primary text-primary-foreground text-xs font-bold shrink-0">
            S
          </div>
          {!collapsed && <span className="font-bold text-lg truncate">i-rs-schedule</span>}
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Navigation</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {nav.map(({ to, label, icon: Icon }) => {
                  const link = (
                    <NavLink
                      to={to}
                      end={to === "/"}
                      onClick={handleClick}
                      className={({ isActive }) =>
                        cn(
                          "flex rounded-md text-sm transition-colors",
                          "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                          "[&_svg]:size-4 [&_svg]:shrink-0",
                          collapsed
                            ? "size-8 items-center justify-center p-0"
                            : "w-full items-center gap-2 p-2",
                          isActive
                            ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
                            : "text-sidebar-foreground/70"
                        )
                      }
                    >
                      <Icon />
                      {!collapsed && <span className="truncate">{label}</span>}
                    </NavLink>
                  );

                  return (
                    <SidebarMenuItem key={to}>
                      {collapsed ? (
                        <Tooltip>
                          <TooltipTrigger render={link} />
                          <TooltipContent side="right">{label}</TooltipContent>
                        </Tooltip>
                      ) : (
                        link
                      )}
                    </SidebarMenuItem>
                  );
                })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  );
}

function App() {
  const { theme, toggle } = useTheme();

  return (
    <TooltipProvider>
      <ToastContainer />
      <BrowserRouter>
        <DocumentTitle />
        <SidebarProvider defaultOpen>
          <div className="flex min-h-screen w-full">
            <SidebarNav />

            <main className="flex-1 flex flex-col min-w-0">
              <header className="sticky top-0 z-40 flex items-center gap-3 border-b border-border/50 bg-background/70 px-4 py-3 backdrop-blur-md">
                <SidebarTrigger />
                <span className="hidden sm:inline"><HeaderPageName /></span>
                <span className="font-bold text-lg tracking-tight md:hidden">i-rs-schedule</span>
                <div className="flex-1" />
                <Button size="icon-sm" variant="ghost" onClick={toggle}>
                  {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
                </Button>
              </header>
              <div className="flex-1 p-4 md:p-8 overflow-auto">
                <ErrorBoundary>
                  <Suspense
                    fallback={
                      <div className="flex h-full items-center justify-center text-muted-foreground">
                        <RefreshCw className="h-5 w-5 animate-spin" />
                      </div>
                    }
                  >
                    <Routes>
                      <Route path="/" element={<Dashboard />} />
                      <Route path="/tasks" element={<Tasks />} />
                      <Route path="/executions" element={<Executions />} />
                    </Routes>
                  </Suspense>
                </ErrorBoundary>
              </div>
            </main>
          </div>
        </SidebarProvider>
      </BrowserRouter>
    </TooltipProvider>
  );
}

export default App;
