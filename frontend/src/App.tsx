import { BrowserRouter, Routes, Route, NavLink, useLocation } from "react-router-dom";
import Dashboard from "@/pages/Dashboard";
import Tasks from "@/pages/Tasks";
import Executions from "@/pages/Executions";
import { LayoutDashboard, ListTodo, ScrollText, Sun, Moon } from "lucide-react";
import { useTheme } from "@/hooks/useTheme";
import { SidebarProvider, Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarTrigger, useSidebar } from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/executions", label: "Executions", icon: ScrollText },
];

function SidebarNav() {
  const location = useLocation();
  const { setOpenMobile } = useSidebar();

  const handleClick = () => setOpenMobile(false);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className="px-3 py-2 font-bold text-lg">i-rs-schedule</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Navigation</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {nav.map(({ to, label, icon: Icon }) => (
                <SidebarMenuItem key={to}>
                  <SidebarMenuButton
                    isActive={to === "/" ? location.pathname === "/" : location.pathname.startsWith(to)}
                    tooltip={label}
                    render={<NavLink to={to} end={to === "/"} onClick={handleClick} />}
                  >
                    <Icon />
                    <span>{label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
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
