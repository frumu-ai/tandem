import matplotlib.pyplot as plt
import pandas as pd
import numpy as np

# Standard Matplotlib Template
# Use this for publication-quality static charts

# 1. Setup Data
# Replace with your data loading logic
# df = pd.read_csv('your_data.csv')
data = {
    'Category': ['A', 'B', 'C', 'D', 'E'],
    'Value': [25, 40, 30, 45, 10]
}
df = pd.DataFrame(data)

# 2. Configure Style
plt.style.use('seaborn-v0_8-whitegrid')
plt.rcParams.update({
    'figure.figsize': (10, 6),
    'figure.dpi': 150,
    'font.size': 11,
    'axes.titlesize': 14,
    'axes.titleweight': 'bold',
})

# 3. Create Plot
fig, ax = plt.subplots()

# Example: Bar Chart
bars = ax.bar(df['Category'], df['Value'], color='#4C72B0', alpha=0.9)

# 4. Add Labels and Title
ax.set_title('Metric Performance by Category')
ax.set_xlabel('Category')
ax.set_ylabel('Value')

# 5. Final Polish
ax.spines['top'].set_visible(False)
ax.spines['right'].set_visible(False)
ax.grid(axis='x', alpha=0) # Hide vertical grid lines

# Add value labels on top of bars
for bar in bars:
    height = bar.get_height()
    ax.text(bar.get_x() + bar.get_width()/2., height,
            f'{height}',
            ha='center', va='bottom')

plt.tight_layout()

# 6. Save or Show
# plt.savefig('output_chart.png', dpi=150, bbox_inches='tight')
plt.show()
