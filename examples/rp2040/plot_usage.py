import re
import matplotlib.pyplot as plt

data = []
time_ms = 0
interval = 200  # Match with MEM_USAGE_TRACK_INTERVAL in src/main.rs

with open("log.txt") as f:
    for line in f:
        match = re.search(r"mem_usage: (\d+)", line)
        if match:
            data.append((time_ms, int(match.group(1))))
            time_ms += interval

times, usage = zip(*data)
plt.plot(times, usage)
plt.xlabel("Time (ms)")
plt.ylabel("Heap Usage (bytes)")
plt.title("Embedded Heap Usage Over Time")
plt.grid(True)
plt.savefig("plot.png")
plt.show()
