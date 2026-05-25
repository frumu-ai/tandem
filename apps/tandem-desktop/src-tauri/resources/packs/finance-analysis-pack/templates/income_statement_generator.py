import pandas as pd
import numpy as np

# Income Statement Generator
# Generates a professional P&L from transaction data

# 1. Setup Data
# Replace with your actual data source
# df = pd.read_csv('../inputs/transactions.csv')

# Sample data generation
data = {
    'Account': ['Revenue', 'Revenue', 'COGS', 'COGS', 'OpEx', 'OpEx', 'OpEx', 'Tax'],
    'Sub-Account': ['Product Sales', 'Service Revenue', 'Material Cost', 'Labor Cost', 'Marketing', 'R&D', 'G&A', 'Income Tax'],
    'Amount': [500000, 120000, -200000, -80000, -45000, -60000, -30000, -40000],
    'Period': ['2023-Q1'] * 8
}
df = pd.DataFrame(data)

# 2. Define Structure
structure = {
    'Revenue': ['Product Sales', 'Service Revenue'],
    'Cost of Goods Sold': ['Material Cost', 'Labor Cost'],
    'Gross Profit': [], # Calculated
    'Operating Expenses': ['Marketing', 'R&D', 'G&A'],
    'Operating Income': [], # Calculated
    'Taxes': ['Income Tax'],
    'Net Income': [] # Calculated
}

# 3. Calculate Totals
def get_total(df, sub_accounts):
    return df[df['Sub-Account'].isin(sub_accounts)]['Amount'].sum()

revenue = get_total(df, structure['Revenue'])
cogs = get_total(df, structure['Cost of Goods Sold'])
gross_profit = revenue + cogs # cogs is negative

opex = get_total(df, structure['Operating Expenses'])
operating_income = gross_profit + opex # opex is negative

taxes = get_total(df, structure['Taxes'])
net_income = operating_income + taxes # taxes is negative

# 4. Create Statement DataFrame
statement_data = [
    {'Line Item': 'Revenue', 'Amount': revenue, 'Type': 'Header'},
    {'Line Item': '  Product Sales', 'Amount': get_total(df, ['Product Sales']), 'Type': 'Detail'},
    {'Line Item': '  Service Revenue', 'Amount': get_total(df, ['Service Revenue']), 'Type': 'Detail'},
    {'Line Item': 'Cost of Goods Sold', 'Amount': cogs, 'Type': 'Header'},
    {'Line Item': 'Gross Profit', 'Amount': gross_profit, 'Type': 'Total'},
    {'Line Item': 'Operating Expenses', 'Amount': opex, 'Type': 'Header'},
    {'Line Item': 'Operating Income', 'Amount': operating_income, 'Type': 'Total'},
    {'Line Item': 'Net Income', 'Amount': net_income, 'Type': 'Grand Total'},
]
statement_df = pd.DataFrame(statement_data)

# 5. Format and Export
print("Income Statement (2023-Q1)")
print("-" * 40)
for _, row in statement_df.iterrows():
    amount_str = f"${abs(row['Amount']):,.2f}"
    if row['Amount'] < 0 and row['Type'] != 'Total': # Display negative numbers in parentheses if preferred, but here we used signed math
        amount_str = f"(${abs(row['Amount']):,.2f})"
    
    if row['Type'] == 'Grand Total':
        print(f"{row['Line Item'].ljust(25)} {amount_str.rjust(15)}")
        print("=" * 40)
    elif row['Type'] == 'Total':
        print(f"{row['Line Item'].ljust(25)} {amount_str.rjust(15)}")
        print("-" * 40)
    else:
        print(f"{row['Line Item'].ljust(25)} {amount_str.rjust(15)}")

# statement_df.to_excel('income_statement.xlsx', index=False)
