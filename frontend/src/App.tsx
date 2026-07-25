import { BrowserRouter, Routes, Route, NavLink } from "react-router-dom";
import Dashboard from "@/pages/Dashboard";
import Tasks from "@/pages/Tasks";
import Executions from "@/pages/Executions";
import { LayoutDashboard, ListTodo, ScrollText, Sun, Moon } from "lucide-react";
import { useTheme } from "@/hooks/useTheme";
import { SidebarProvider, Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuItem, SidebarTrigger, useSidebar } from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { TooltipProvider, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/executions", label: "Executions", icon: ScrollText },
];

function SidebarNav() {
  const { setOpenMobile, state } = useSidebar();
  const collapsed = state === "collapsed";

  const handleClick = () => setOpenMobile(false);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className="flex items-center gap-2 px-3 py-2">
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
      <BrowserRouter>
        <SidebarProvider defaultOpen>
          <div className="flex min-h-screen w-full bg-muted/30">
            <SidebarNav />

            <main className="flex-1 flex flex-col min-w-0">
              <header className="sticky top-0 z-40 flex items-center gap-3 border-b bg-background px-4 py-3">
                <SidebarTrigger />
                <span className="font-bold text-lg md:hidden">i-rs-schedule</span>
                <div className="flex-1" />
                <Button size="icon-sm" variant="ghost" onClick={toggle}>
                  {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
                </Button>
              </header>
              <div className="flex-1 p-4 md:p-6 overflow-auto">
                <Routes>
                  <Route path="/" element={<Dashboard />} />
                  <Route path="/tasks" element={<Tasks />} />
                  <Route path="/executions" element={<Executions />} />
                </Routes>
              </div>
            </main>
          </div>
        </SidebarProvider>
      </BrowserRouter>
    </TooltipProvider>
  );
}

export default App;
