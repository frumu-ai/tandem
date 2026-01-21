google canvas can build and display these right inside its web features. research how we can implement this as a tool for tandem!

<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Global Short-Term Rental Atlas: 2025 Investment Guide</title>
    <!-- Tailwind CSS -->
    <script src="https://cdn.tailwindcss.com"></script>
    <!-- Chart.js -->
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <!-- Font Awesome -->
    <link href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" rel="stylesheet">
    <!-- Google Fonts -->
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&display=swap" rel="stylesheet">

    <style>
        body {
            font-family: 'Inter', sans-serif;
            background-color: #F8FAFC;
            color: #0F172A;
        }

        /* Color System: "Global Finance" */
        :root {
            --primary: #0F172A;     /* Navy */
            --secondary: #3B82F6;   /* Bright Blue */
            --accent: #10B981;      /* Yield Green */
            --warning: #F59E0B;     /* Moderate Orange */
            --danger: #EF4444;      /* Expensive Red */
        }

        /* Chart Containers */
        .chart-container {
            position: relative;
            width: 100%;
            height: 350px;
        }
        .chart-container-lg {
            position: relative;
            width: 100%;
            height: 500px;
        }

        /* Table Styling */
        .data-table-container {
            max-height: 800px; /* Scrollable large list */
            overflow-y: auto;
            border: 1px solid #E2E8F0;
            border-radius: 0.75rem;
        }
        .data-table th {
            background-color: #F1F5F9;
            position: sticky;
            top: 0;
            z-index: 20;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: #64748B;
        }
        .data-table tr:hover {
            background-color: #F0F9FF;
            transition: background-color 0.15s;
        }

        /* Utility Classes */
        .yield-badge {
            display: inline-flex;
            align-items: center;
            padding: 4px 8px;
            border-radius: 9999px;
            font-weight: 700;
            font-size: 0.75rem;
        }
        .yield-stellar { background-color: #D1FAE5; color: #065F46; } /* >9% */
        .yield-good { background-color: #ECFDF5; color: #047857; }    /* 6-9% */
        .yield-avg { background-color: #FEF3C7; color: #92400E; }     /* 4-6% */
        .yield-low { background-color: #FEE2E2; color: #991B1B; }     /* <4% */

        /* Custom Scrollbar for table */
        .data-table-container::-webkit-scrollbar {
            width: 8px;
        }
        .data-table-container::-webkit-scrollbar-track {
            background: #f1f1f1;
        }
        .data-table-container::-webkit-scrollbar-thumb {
            background: #94A3B8;
            border-radius: 4px;
        }
    </style>
</head>
<body class="antialiased">

    <!-- Header -->
    <header class="bg-[#0F172A] text-white py-12 px-6 shadow-2xl border-b-4 border-[#3B82F6]">
        <div class="max-w-7xl mx-auto flex flex-col md:flex-row justify-between items-center">
            <div class="mb-6 md:mb-0">
                <h1 class="text-3xl md:text-5xl font-bold tracking-tight">Global Rental Atlas</h1>
                <p class="text-blue-200 mt-2 text-lg">Investment Guide: Research, Analyze, and Acquire</p>
            </div>
            <div class="flex space-x-8 text-center">
                <div>
                    <div class="text-[#10B981] text-3xl font-bold">50+</div>
                    <div class="text-xs text-gray-400 uppercase">Markets Analyzed</div>
                </div>
                <div>
                    <div class="text-[#3B82F6] text-3xl font-bold">€164</div>
                    <div class="text-xs text-gray-400 uppercase">Global Avg Rate</div>
                </div>
            </div>
        </div>
    </header>

    <main class="max-w-7xl mx-auto px-4 py-10 space-y-16">

        <!-- Section 1: The Global Yield Landscape (Scatter Plot) -->
        <section class="bg-white rounded-xl shadow-lg p-6 border border-gray-100">
            <div class="mb-6 flex flex-col md:flex-row justify-between items-end">
                <div>
                    <h2 class="text-2xl font-bold text-[#0F172A]">Global ROI Landscape</h2>
                    <p class="text-gray-500 text-sm mt-1">
                        Compare Purchase Price (X-Axis) vs. Annual Revenue (Y-Axis). 
                        <br>
                        <span class="text-[#10B981] font-bold">Top Left Quadrant</span> represents the "High Yield Zone" (Low Cost, High Income).
                    </p>
                </div>
                <div class="flex space-x-2 text-xs font-semibold mt-4 md:mt-0">
                    <span class="flex items-center"><span class="w-3 h-3 rounded-full bg-[#3B82F6] mr-1"></span> Europe</span>
                    <span class="flex items-center"><span class="w-3 h-3 rounded-full bg-[#F59E0B] mr-1"></span> Americas</span>
                    <span class="flex items-center"><span class="w-3 h-3 rounded-full bg-[#10B981] mr-1"></span> Asia/MEA</span>
                </div>
            </div>
            <div class="chart-container-lg">
                <canvas id="globalScatterChart"></canvas>
            </div>
        </section>

        <!-- Section 2: Property Dynamics (Efficiency & Location) -->
        <section class="grid grid-cols-1 lg:grid-cols-2 gap-8">
            
            <!-- Analysis A: Distance Decay (Downtown vs Suburbs) -->
            <div class="bg-white rounded-xl shadow-md p-6">
                <h3 class="text-xl font-bold text-[#0F172A] mb-2"><i class="fas fa-bullseye text-[#EF4444] mr-2"></i>The "Downtown Premium"</h3>
                <p class="text-sm text-gray-500 mb-6">
                    Across 90% of global markets, revenue drops significantly as you move away from the city center. However, the purchase price often drops faster, creating yield opportunities in the 2-5km ring.
                </p>
                <div class="chart-container">
                    <canvas id="distanceDecayChart"></canvas>
                </div>
            </div>

            <!-- Analysis B: Size Efficiency (Sq Meters vs Revenue) -->
            <div class="bg-white rounded-xl shadow-md p-6">
                <h3 class="text-xl font-bold text-[#0F172A] mb-2"><i class="fas fa-ruler-combined text-[#3B82F6] mr-2"></i>Space Efficiency</h3>
                <p class="text-sm text-gray-500 mb-6">
                    While 3+ bedroom homes generate high nightly rates, <strong>Studios and 1-Bedrooms</strong> (35-55m²) typically generate the highest revenue <em>per square meter</em> due to higher occupancy consistency.
                </p>
                <div class="chart-container">
                    <canvas id="sizeEfficiencyChart"></canvas>
                </div>
            </div>
        </section>

        <!-- Section 3: The Exhaustive Database -->
        <section id="database" class="scroll-mt-24">
            <div class="flex flex-col md:flex-row justify-between items-center mb-6">
                <div>
                    <h2 class="text-2xl font-bold text-[#0F172A]">Comprehensive Market Database</h2>
                    <p class="text-sm text-gray-500">Searchable list of 50+ global locations. Prices in Euro (€).</p>
                </div>
                
                <!-- Filter Controls -->
                <div class="flex space-x-2 mt-4 md:mt-0 bg-gray-100 p-1 rounded-lg">
                    <button onclick="filterTable('all')" class="px-4 py-2 text-sm font-semibold rounded-md bg-white shadow-sm text-blue-600 focus:outline-none hover:bg-gray-50 transition">All</button>
                    <button onclick="filterTable('Europe')" class="px-4 py-2 text-sm font-semibold rounded-md text-gray-600 hover:bg-white hover:shadow-sm transition focus:outline-none">Europe</button>
                    <button onclick="filterTable('Americas')" class="px-4 py-2 text-sm font-semibold rounded-md text-gray-600 hover:bg-white hover:shadow-sm transition focus:outline-none">Americas</button>
                    <button onclick="filterTable('Asia')" class="px-4 py-2 text-sm font-semibold rounded-md text-gray-600 hover:bg-white hover:shadow-sm transition focus:outline-none">Asia/MEA</button>
                </div>
            </div>

            <div class="data-table-container bg-white shadow-lg">
                <table class="w-full data-table text-left border-collapse">
                    <thead>
                        <tr>
                            <th class="p-4">Region</th>
                            <th class="p-4">Location</th>
                            <th class="p-4 hidden md:table-cell">Property Profile</th>
                            <th class="p-4 hidden md:table-cell">Avg Size</th>
                            <th class="p-4">Est. Price (€)</th>
                            <th class="p-4">Est. Income (€/yr)</th>
                            <th class="p-4 text-center">Gross Yield</th>
                            <th class="p-4 text-right w-40">Investment Actions</th>
                        </tr>
                    </thead>
                    <tbody id="marketTableBody" class="text-sm text-gray-700 divide-y divide-gray-100">
                        <!-- Javascript will populate this -->
                    </tbody>
                </table>
            </div>
        </section>

        <!-- Footer -->
        <footer class="bg-[#0F172A] text-white py-12 rounded-t-xl mt-12">
            <div class="text-center max-w-4xl mx-auto px-4">
                <h3 class="text-xl font-bold mb-4">Investment Disclaimer</h3>
                <p class="text-gray-400 text-sm mb-6">
                    Data provided is for educational research purposes only and represents estimated market averages for renovated, well-managed properties as of 2025. Purchase prices vary significantly by specific neighborhood. Income estimates assume 60-75% occupancy.
                </p>
                <p class="text-xs text-gray-600">&copy; 2025 Global Rental Research. All Rights Reserved.</p>
            </div>
        </footer>

    </main>

    <!-- Logic & Data -->
    <script>
        // --- 1. THE EXHAUSTIVE DATASET ---
        // Normalized to Euros (€) for comparison. 
        // Regions: 'Europe', 'Americas', 'Asia' (Includes SE Asia, Middle East/Africa)
        const marketData = [
            // --- EUROPE ---
            { city: "Budapest", country: "Hungary", region: "Europe", type: "City Flat (Dist V/VI)", size: 55, price: 195000, income: 28500 },
            { city: "Krakow", country: "Poland", region: "Europe", type: "Old Town Apt", size: 50, price: 175000, income: 24000 },
            { city: "Prague", country: "Czechia", region: "Europe", type: "City Flat", size: 55, price: 260000, income: 32000 },
            { city: "Warsaw", country: "Poland", region: "Europe", type: "Modern Condo", size: 50, price: 210000, income: 26500 },
            { city: "Bucharest", country: "Romania", region: "Europe", type: "City Flat", size: 60, price: 140000, income: 19000 },
            { city: "Tallinn", country: "Estonia", region: "Europe", type: "Old Town", size: 50, price: 200000, income: 23000 },
            { city: "Malaga", country: "Spain", region: "Europe", type: "Coastal Apt", size: 65, price: 340000, income: 38000 },
            { city: "Barcelona", country: "Spain", region: "Europe", type: "City Flat", size: 55, price: 420000, income: 45000 },
            { city: "Split", country: "Croatia", region: "Europe", type: "Sea View", size: 60, price: 290000, income: 31000 },
            { city: "Lisbon", country: "Portugal", region: "Europe", type: "Hillside Apt", size: 60, price: 390000, income: 41000 },
            { city: "Faro (Algarve)", country: "Portugal", region: "Europe", type: "Resort Condo", size: 80, price: 310000, income: 34000 },
            { city: "Athens", country: "Greece", region: "Europe", type: "Acropolis View", size: 55, price: 220000, income: 29000 },
            { city: "Nice", country: "France", region: "Europe", type: "Luxury Apt", size: 50, price: 480000, income: 42000 },
            { city: "Paris", country: "France", region: "Europe", type: "Studio", size: 35, price: 450000, income: 36000 },
            { city: "London", country: "UK", region: "Europe", type: "Zone 2 Flat", size: 50, price: 600000, income: 45000 },
            { city: "Edinburgh", country: "UK", region: "Europe", type: "Historic Flat", size: 55, price: 350000, income: 42000 },
            { city: "Dublin", country: "Ireland", region: "Europe", type: "City Apt", size: 55, price: 400000, income: 38000 },
            { city: "Berlin", country: "Germany", region: "Europe", type: "Loft", size: 60, price: 380000, income: 32000 },
            { city: "Munich", country: "Germany", region: "Europe", type: "Modern Flat", size: 55, price: 480000, income: 30000 },
            { city: "Vienna", country: "Austria", region: "Europe", type: "Altbau", size: 65, price: 420000, income: 34000 },
            { city: "Amsterdam", country: "Netherlands", region: "Europe", type: "Canal House", size: 50, price: 550000, income: 40000 },
            { city: "Rome", country: "Italy", region: "Europe", type: "City Apt", size: 55, price: 440000, income: 38000 },
            { city: "Milan", country: "Italy", region: "Europe", type: "Fashion Dist", size: 50, price: 460000, income: 36000 },

            // --- SOUTHEAST ASIA & EAST ASIA (Expanded) ---
            { city: "Bangkok", country: "Thailand", region: "Asia", type: "Sukhumvit Condo", size: 45, price: 150000, income: 14000 },
            { city: "Phuket", country: "Thailand", region: "Asia", type: "Sea View Condo", size: 60, price: 180000, income: 21000 },
            { city: "Chiang Mai", country: "Thailand", region: "Asia", type: "Nimman Condo", size: 45, price: 80000, income: 12000 },
            { city: "Koh Samui", country: "Thailand", region: "Asia", type: "Pool Villa", size: 120, price: 250000, income: 35000 },
            { city: "Ho Chi Minh City", country: "Vietnam", region: "Asia", type: "Dist 1 Apt", size: 50, price: 180000, income: 20000 },
            { city: "Hanoi", country: "Vietnam", region: "Asia", type: "Old Quarter", size: 40, price: 150000, income: 18000 },
            { city: "Da Nang", country: "Vietnam", region: "Asia", type: "Beachfront", size: 60, price: 130000, income: 16000 },
            { city: "Bali (Canggu)", country: "Indonesia", region: "Asia", type: "Villa", size: 150, price: 250000, income: 35000 },
            { city: "Ubud", country: "Indonesia", region: "Asia", type: "Jungle Villa", size: 100, price: 210000, income: 28000 },
            { city: "Kuala Lumpur", country: "Malaysia", region: "Asia", type: "KLCC Condo", size: 70, price: 160000, income: 15000 },
            { city: "Manila", country: "Philippines", region: "Asia", type: "Makati Condo", size: 35, price: 120000, income: 14000 },
            { city: "Tokyo", country: "Japan", region: "Asia", type: "Shinjuku Apt", size: 30, price: 350000, income: 28000 },
            { city: "Seoul", country: "South Korea", region: "Asia", type: "Gangnam Apt", size: 40, price: 400000, income: 28000 },
            { city: "Sydney", country: "Australia", region: "Asia", type: "Bondi Flat", size: 60, price: 700000, income: 50000 },

            // --- MEA (Middle East / Africa) ---
            { city: "Dubai", country: "UAE", region: "Asia", type: "Marina Apt", size: 75, price: 380000, income: 42000 },
            { city: "Cape Town", country: "South Africa", region: "Asia", type: "City Bowl", size: 70, price: 220000, income: 28000 },
            { city: "Marrakech", country: "Morocco", region: "Asia", type: "Riad Room", size: 40, price: 160000, income: 18000 },

            // --- AMERICAS ---
            { city: "New York", country: "USA", region: "Americas", type: "Manhattan Condo", size: 50, price: 850000, income: 65000 },
            { city: "Los Angeles", country: "USA", region: "Americas", type: "Bungalow", size: 70, price: 750000, income: 58000 },
            { city: "Sevierville", country: "USA", region: "Americas", type: "Smokies Cabin", size: 120, price: 450000, income: 55000 },
            { city: "Scottsdale", country: "USA", region: "Americas", type: "Desert Home", size: 150, price: 600000, income: 70000 },
            { city: "Miami", country: "USA", region: "Americas", type: "South Beach Apt", size: 60, price: 550000, income: 48000 },
            { city: "Austin", country: "USA", region: "Americas", type: "East Side House", size: 100, price: 450000, income: 42000 },
            { city: "Tulum", country: "Mexico", region: "Americas", type: "Jungle Condo", size: 70, price: 220000, income: 28000 },
            { city: "Mexico City", country: "Mexico", region: "Americas", type: "Condesa Apt", size: 80, price: 280000, income: 32000 },
            { city: "Rio de Janeiro", country: "Brazil", region: "Americas", type: "Copacabana", size: 60, price: 180000, income: 22000 }
        ];

        // --- 2. TABLE LOGIC ---
        function formatCurrency(num) {
            return '€' + num.toLocaleString();
        }

        function calculateYield(price, income) {
            return ((income / price) * 100).toFixed(1);
        }

        function getYieldClass(yieldVal) {
            if (yieldVal >= 9) return 'yield-stellar';
            if (yieldVal >= 6) return 'yield-good';
            if (yieldVal >= 4) return 'yield-avg';
            return 'yield-low';
        }

        function renderTable(filterRegion = 'all') {
            const tbody = document.getElementById('marketTableBody');
            tbody.innerHTML = '';

            // Sort by Yield (Descending) automatically for better UX
            const sortedData = [...marketData].sort((a, b) => {
                return (b.income / b.price) - (a.income / a.price);
            });

            sortedData.forEach(item => {
                if (filterRegion !== 'all' && item.region !== filterRegion) return;

                const grossYield = calculateYield(item.price, item.income);
                
                // Dynamic Search URLs
                const searchListingsUrl = `https://www.airbnb.com/s/${encodeURIComponent(item.city + ', ' + item.country)}/homes`;
                const mapUrl = `https://www.google.com/maps/search/?api=1&query=${encodeURIComponent(item.city + ', ' + item.country)}`;
                const buyUrl = `https://www.google.com/search?q=${encodeURIComponent('buy property in ' + item.city + ' ' + item.country + ' real estate')}`;

                const row = document.createElement('tr');
                row.className = "border-b border-gray-100 hover:bg-blue-50 transition";
                row.innerHTML = `
                    <td class="p-4 text-xs font-bold text-gray-400 uppercase">${item.region}</td>
                    <td class="p-4">
                        <div class="font-bold text-[#0F172A]">${item.city}</div>
                        <div class="text-xs text-gray-500">${item.country}</div>
                    </td>
                    <td class="p-4 hidden md:table-cell text-sm text-gray-600">${item.type}</td>
                    <td class="p-4 hidden md:table-cell text-sm font-mono text-gray-500">${item.size} m²</td>
                    <td class="p-4 font-mono text-gray-700">${formatCurrency(item.price)}</td>
                    <td class="p-4 font-mono text-[#10B981] font-semibold">${formatCurrency(item.income)}</td>
                    <td class="p-4 text-center">
                        <span class="yield-badge ${getYieldClass(grossYield)}">${grossYield}%</span>
                    </td>
                    <td class="p-4 text-right space-x-1 whitespace-nowrap">
                        <a href="${mapUrl}" target="_blank" title="View on Map" class="inline-flex items-center justify-center w-8 h-8 rounded-full bg-gray-100 text-gray-600 hover:bg-gray-600 hover:text-white transition shadow-sm">
                            <i class="fas fa-map-marker-alt"></i>
                        </a>
                        <a href="${searchListingsUrl}" target="_blank" title="Check Airbnb Listings" class="inline-flex items-center justify-center w-8 h-8 rounded-full bg-blue-100 text-blue-600 hover:bg-blue-600 hover:text-white transition shadow-sm">
                            <i class="fas fa-search"></i>
                        </a>
                        <a href="${buyUrl}" target="_blank" title="Find Properties to Buy" class="inline-flex items-center justify-center w-8 h-8 rounded-full bg-purple-100 text-purple-600 hover:bg-purple-600 hover:text-white transition shadow-sm">
                            <i class="fas fa-home"></i>
                        </a>
                    </td>
                `;
                tbody.appendChild(row);
            });
        }

        function filterTable(region) {
            renderTable(region);
        }

        // --- 3. CHART VISUALIZATIONS ---
        
        // A. GLOBAL SCATTER PLOT
        const ctxGlobal = document.getElementById('globalScatterChart').getContext('2d');
        const scatterPoints = marketData.map(item => ({
            x: item.price,
            y: item.income,
            city: item.city,
            region: item.region,
            yield: calculateYield(item.price, item.income)
        }));

        new Chart(ctxGlobal, {
            type: 'scatter',
            data: {
                datasets: [{
                    label: 'Investment Markets',
                    data: scatterPoints,
                    backgroundColor: (ctx) => {
                        const r = ctx.raw?.region;
                        if (r === 'Europe') return '#3B82F6'; // Blue
                        if (r === 'Americas') return '#F59E0B'; // Orange
                        return '#10B981'; // Green (Asia/MEA)
                    },
                    pointRadius: 6,
                    pointHoverRadius: 10
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                scales: {
                    x: {
                        type: 'linear',
                        position: 'bottom',
                        title: { display: true, text: 'Purchase Price (€)' },
                        grid: { color: '#F1F5F9' },
                        ticks: { callback: (val) => '€' + val/1000 + 'k' }
                    },
                    y: {
                        title: { display: true, text: 'Annual Revenue (€)' },
                        grid: { color: '#F1F5F9' },
                        ticks: { callback: (val) => '€' + val/1000 + 'k' }
                    }
                },
                plugins: {
                    tooltip: {
                        callbacks: {
                            label: (ctx) => {
                                const p = ctx.raw;
                                return `${p.city}: Cost €${p.x.toLocaleString()} -> Earn €${p.y.toLocaleString()} (${p.yield}%)`;
                            }
                        }
                    }
                }
            }
        });

        // B. DISTANCE DECAY (Data from previous research)
        const ctxDist = document.getElementById('distanceDecayChart').getContext('2d');
        new Chart(ctxDist, {
            type: 'bar',
            data: {
                labels: ['City Center (<1km)', 'Inner Ring (1-3km)', 'Outer Ring (3-6km)', 'Suburbs (6-10km)', 'Periphery (10km+)'],
                datasets: [{
                    label: 'Avg Nightly Rate (€)',
                    data: [230, 180, 145, 120, 100],
                    backgroundColor: ['#0F172A', '#334155', '#475569', '#64748B', '#94A3B8'],
                    borderRadius: 4
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                scales: {
                    x: { grid: { display: false }, ticks: { font: { size: 10 } } },
                    y: { beginAtZero: true }
                },
                plugins: { legend: { display: false } }
            }
        });

        // C. SIZE EFFICIENCY (Data from previous research)
        const ctxSize = document.getElementById('sizeEfficiencyChart').getContext('2d');
        new Chart(ctxSize, {
            type: 'line',
            data: {
                labels: ['Studio (35m²)', '1-Bed (55m²)', '2-Bed (85m²)', '3-Bed (120m²)', '4-Bed (180m²)'],
                datasets: [{
                    label: 'Revenue per Sq Meter (€)',
                    data: [1028, 980, 850, 720, 610], // Calculated metric
                    borderColor: '#10B981',
                    backgroundColor: 'rgba(16, 185, 129, 0.1)',
                    fill: true,
                    tension: 0.4,
                    pointRadius: 6,
                    pointBackgroundColor: '#fff',
                    pointBorderWidth: 2
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                scales: {
                    y: { beginAtZero: false, title: { display: true, text: 'Rev per m²' } },
                    x: { grid: { display: false } }
                },
                plugins: { legend: { display: false } }
            }
        });

        // --- UTILITY: Wrap Label Logic (Standard requirement) ---
        function wrapLabel(str, maxChars) {
            if (str.length <= maxChars) return str;
            const words = str.split(' ');
            const lines = [];
            let currentLine = words[0];
            for (let i = 1; i < words.length; i++) {
                if (currentLine.length + 1 + words[i].length <= maxChars) {
                    currentLine += ' ' + words[i];
                } else {
                    lines.push(currentLine);
                    currentLine = words[i];
                }
            }
            lines.push(currentLine);
            return lines;
        }

        // Initialize Table
        renderTable();

    </script>
</body>
</html>