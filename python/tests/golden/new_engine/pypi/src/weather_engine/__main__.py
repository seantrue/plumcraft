"""`python -m weather_engine`: this engine as a process. PlumCraft starts it with
`PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN` set; `--listen 127.0.0.1:7311` serves HTTP,
for Engine › Attach and `plumcraft-engine run --attach`. `weather-engine register` offers the installed
engine to every PlumCraft editor, and `weather-engine unregister` withdraws it."""

import sys

from weather_engine import WeatherEngine


def main() -> int:
    return WeatherEngine.serve()


if __name__ == "__main__":
    sys.exit(main())
