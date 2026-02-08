import pandas as pd
import numpy as np

# Variance Analysis Template
# Compares Actual vs Budget and calculates variances

# 1. Setup Data
# df_actual = pd.read_csv('actuals.csv')
# df_budget = pd.read_csv('budget.csv')

data = {
    'Department': ['Sales', 'Marketing', 'Engineering', 'HR', 'Finance'],
    'Actual': [120000, 45000, 150000, 20000, 25000],
    'Budget': [100000, 50000, 140000, 20000, 22000]
}
df = pd.DataFrame(data)

# 2. Calculate Variance
df['Variance ($)'] = df['Actual'] - df['Budget']
df['Variance (%)'] = (df['Variance ($)'] / df['Budget']) * 100

# 3. Flag Significant Variances
# Threshold: +/- 10%
df['Status'] = df['Variance (%)'].apply(
    lambda x: 'Red' if x > 10 else ('Green' if x < -10 else 'Yellow')
)
# Note: Logic depends on if higher is better (Revenue) or worse (Expenses). 
# Assuming Expenses here (Higher Actual = Bad/Red).

# 4. Create Summary
print("Variance Analysis Report")
print("=" * 60)
print(f"{'Department':<15} {'Actual':>10} {'Budget':>10} {'Var ($)':>10} {'Var (%)':>10} {'Status':>8}")
print("-" * 60)

for _, row in df.iterrows():
    print(f"{row['Department']:<15} "
          f"${row['Actual']:,.0f}".rjust(10) + " " +
          f"${row['Budget']:,.0f}".rjust(10) + " " +
          f"${row['Variance ($)']:,.0f}".rjust(10) + " " +
          f"{row['Variance (%)']:.1f}%".rjust(10) + " " +
          f"{row['Status']:>8}")

# 5. Export
# df.to_excel('variance_analysis.xlsx', index=False)
