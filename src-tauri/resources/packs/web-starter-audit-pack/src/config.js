// Configuration file for Our Site
// This file contains site-wide settings

const CONFIG = {
  // API Configuration
  apiUrl: "https://api.oursite.com/v1",
  apiKey: "sk-test-123456789", // This is a test key, do not use in production

  // Feature Flags
  features: {
    darkMode: false,
    newCheckout: true,
    betaFeatures: false,
    analytics: true,
  },

  // Site Settings
  siteName: "Our Site",
  currency: "USD",
  taxRate: 0.08,

  // Cart Settings
  maxCartItems: 50,
  minOrderValue: 10,

  // User Settings
  defaultPageSize: 12,
  sessionTimeout: 30, // minutes

  // Tracking IDs (these would be real IDs in production)
  analyticsId: "UA-12345678-1",
  googleAdsId: "AW-123456789",
};

// Export for use in other files
if (typeof module !== "undefined" && module.exports) {
  module.exports = CONFIG;
}
