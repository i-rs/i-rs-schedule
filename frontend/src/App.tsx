import { BrowserRouter, Routes, Route, NavLink } from "react-router-dom";
import Dashboard from "@/pages/Dashboard";
import Tasks from "@/pages/Tasks";
import Executions from "@/pages/Executions";
import { LayoutDashboard, ListTodo, ScrollText, Sun, Moon } from "lucide-react";
import { useTheme } from "@/hooks/useTheme";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/executions", label: "Executions", icon: ScrollText },
];

function App() {
  const { theme, toggle } = useTheme();

  return (
    <BrowserRouter>
      <div className="flex min-h-screen bg-muted/30">
        <aside className="w-56 border-r bg-background flex flex-col">
          <div className="p-4 font-bold text-lg border-b">i-rs-schedule</div>
          <nav className="flex-1 p-3 space-y-1">
            {nav.map(({ to, label, icon: Icon }) => (
              <NavLink
                key={to}
                to={to}
                end={to === "/"}
                className={({ isActive }) =>
                  `flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                    isActive ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                  }`
                }
              >
                <Icon className="h-4 w-4" />
                {label}
              </NavLink>
            ))}
          </nav>
          <div className="border-t p-3">
            <button
              onClick={toggle}
              className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors cursor-pointer"
            >
              {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
              {theme === "dark" ? "Light" : "Dark"} mode
            </button>
          </div>
        </aside>

        <main className="flex-1 p-6">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/tasks" element={<Tasks />} />
            <Route path="/executions" element={<Executions />} />
          </Routes>
        </main>
      </div>
    </BrowserRouter>
  );
}

export default App;
