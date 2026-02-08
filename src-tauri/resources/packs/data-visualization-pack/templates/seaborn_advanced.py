import seaborn as sns
import matplotlib.pyplot as plt
import pandas as pd
import numpy as np

# Advanced Seaborn Template
# Use this for statistical visualizations and complex relationships

# 1. Setup Data
# df = pd.read_csv('your_data.csv')
# Generating sample data
np.random.seed(42)
df = pd.DataFrame({
    'x': np.random.normal(size=100),
    'y': np.random.normal(size=100) + np.random.normal(size=100),
    'category': np.random.choice(['Group A', 'Group B'], 100)
})

# 2. Configure Style
sns.set_theme(style="whitegrid", context="talk")
palette = sns.color_palette("husl", 2)

# 3. Create Plot
# Example: Joint Plot with distributions
g = sns.jointplot(
    data=df,
    x="x",
    y="y",
    hue="category",
    palette=palette,
    height=8,
    ratio=5,
    kind="scatter", # options: scatter, kde, hist, hex, reg, resid
    marginal_ticks=True
)

# 4. Customization
g.fig.suptitle('Multivariate Analysis: X vs Y by Group', y=1.02, fontweight='bold')
g.set_axis_labels("Independent Variable (X)", "Dependent Variable (Y)")

# 5. Save or Show
# plt.savefig('seaborn_analysis.png', dpi=150, bbox_inches='tight')
plt.show()
