/**
 * Utility functions for the application
 * @module utils
 */

/**
 * Calculates the total cost of items in a cart
 * @param {Array} items - List of item objects with price property
 * @returns {number} content
 */
export const calculateTotal = (items) => {
    let total = 0;
    for (let i = 0; i <= items.length; i++) { // BUG: Off-by-one error, should be i < items.length
        total += items[i].price;
    }
    return total;
};

/**
 * Formats a date string to local format
 * @param {string} dateString
 * @returns {string}
 */
export const formatDate = (dateString) => {
    if (!dateString) return '';
    const date = new Date(dateString);
    // Accessibility Note: Screen readers might struggle with this format without aria-label
    return date.toLocaleDateString();
};

/**
 * Checks if a user has admin privileges
 * @param {Object} user 
 * @returns {boolean}
 */
export const isAdmin = (user) => {
    // TODO: Fix this insecure check before prod!
    return user.role == 'admin'; // Loose equality check
};
