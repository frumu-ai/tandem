import plotly.express as px
import plotly.graph_objects as go
import pandas as pd

# Interactive Plotly Template
# Use this for web-based, interactive dashboards and exploration

# 1. Setup Data
# df = pd.read_csv('your_data.csv')
data = {
    'Date': pd.date_range(start='2023-01-01', periods=30, freq='D'),
    'Sales': [x + (x*0.1) for x in range(30)],
    'Region': ['North']*15 + ['South']*15
}
df = pd.DataFrame(data)

# 2. Create Plot
# Example: Interactive Line Chart with Range Slider
fig = px.line(
    df, 
    x='Date', 
    y='Sales', 
    color='Region',
    title='Sales Trend Over Time',
    markers=True,
    template='plotly_white'
)

# 3. Customization
fig.update_layout(
    title={
        'text': "Sales Trend Over Time",
        'y':0.95,
        'x':0.5,
        'xanchor': 'center',
        'yanchor': 'top',
        'font': {'size': 24}
    },
    xaxis_title="Date",
    yaxis_title="Sales Volume ($)",
    hovermode="x unified",
    legend=dict(
        yanchor="top",
        y=0.99,
        xanchor="left",
        x=0.01
    )
)

# Add Range Slider
fig.update_xaxes(rangeslider_visible=True)

# 4. Show or Save
# fig.write_html("interactive_chart.html")
fig.show()
