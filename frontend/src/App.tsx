import { BrowserRouter, Routes, Route, NavLink, useLocation } from "react-router-dom";
import Dashboard from "@/pages/Dashboard";
import Tasks from "@/pages/Tasks";
import Executions from "@/pages/Executions";
import { LayoutDashboard, ListTodo, ScrollText, Sun, Moon } from "lucide-react";
import { useTheme } from "@/hooks/useTheme";
import { SidebarProvider, Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarTrigger } from "@/components/ui/sidebar";
import { TooltipProvider } from "@/components/ui/tooltip";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/executions", label: "Executions", icon: ScrollText },
];

function SidebarNav() {
  const location = useLocation();

  return (
    <Sidebar>
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
                    render={<NavLink to={to} end={to === "/"} />}
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
      <SidebarFooter>
        <ThemeToggle />
      </SidebarFooter>
    </Sidebar>
  );
}

function ThemeToggle() {
  const { theme, toggle } = useTheme();
  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <SidebarMenuButton onClick={toggle}>
          {theme === "dark" ? <Sun /> : <Moon />}
          <span>{theme === "dark" ? "Light" : "Dark"} mode</span>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}

function App() {
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
