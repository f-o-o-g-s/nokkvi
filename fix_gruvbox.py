import toml
import shutil
import os

paths = ["themes/gruvbox.toml", os.path.expanduser("~/.config/nokkvi/themes/gruvbox.toml")]

bar_colors = ["#83a598", "#8ec07c", "#b8bb26", "#fabd2f", "#fe8019", "#fb4934"]
peak_colors = ["#83a598", "#83a598", "#b8bb26", "#b8bb26", "#fb4934", "#fb4934"]

for path in paths:
    if os.path.exists(path):
        with open(path, "r") as f:
            data = toml.load(f)
        
        if "dark" in data and "visualizer" in data["dark"]:
            data["dark"]["visualizer"]["bar_gradient_colors"] = bar_colors
            data["dark"]["visualizer"]["peak_gradient_colors"] = peak_colors
        if "light" in data and "visualizer" in data["light"]:
            data["light"]["visualizer"]["bar_gradient_colors"] = bar_colors
            data["light"]["visualizer"]["peak_gradient_colors"] = peak_colors

        with open(path, "w") as f:
            toml.dump(data, f)
        print(f"Fixed {path}")
