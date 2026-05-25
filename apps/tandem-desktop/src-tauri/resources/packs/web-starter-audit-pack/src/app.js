document.addEventListener("DOMContentLoaded", function () {
  const productsContainer = document.getElementById("products");
  const productCards = productsContainer.querySelectorAll(".product-card");
  const filterButtons = document.querySelectorAll(".filter-btn");
  const sortSelect = document.getElementById("sort-select");

  let currentFilter = "all";
  let currentSort = "name-asc";

  function filterProducts() {
    productCards.forEach((card) => {
      const category = card.dataset.category;
      if (currentFilter === "all" || category === currentFilter) {
        card.style.display = "block";
      } else {
        card.style.display = "none";
      }
    });
  }

  function sortProducts() {
    const productsArray = Array.from(productCards);

    productsArray.sort(function (a, b) {
      let valueA, valueB;

      if (currentSort === "name-asc" || currentSort === "name-desc") {
        valueA = a.querySelector("h3").textContent;
        valueB = b.querySelector("h3").textContent;
        return currentSort === "name-asc"
          ? valueA.localeCompare(valueB)
          : valueB.localeCompare(valueA);
      } else if (currentSort === "price-asc" || currentSort === "price-desc") {
        valueA = parseFloat(a.querySelector(".price").textContent.replace("$", ""));
        valueB = parseFloat(b.querySelector(".price").textContent.replace("$", ""));
        return currentSort === "price-asc" ? valueA - valueB : valueB - valueA;
      }
    });

    productsArray.forEach((product) => {
      productsContainer.appendChild(product);
    });
  }

  filterButtons.forEach((button) => {
    button.addEventListener("click", function () {
      filterButtons.forEach((btn) => btn.classList.remove("active"));
      this.classList.add("active");
      currentFilter = this.dataset.filter;
      filterProducts();
    });
  });

  sortSelect.addEventListener("change", function () {
    currentSort = this.value;
    sortProducts();
  });

  const searchInput = document.querySelector(".search-input");
  searchInput.addEventListener("input", function () {
    const searchTerm = this.value.toLowerCase();

    productCards.forEach((card) => {
      const productName = card.querySelector("h3").textContent.toLowerCase();
      const productDesc = card.querySelector("p:last-of-type").textContent.toLowerCase();

      if (productName.includes(searchTerm) || productDesc.includes(searchTerm)) {
        card.style.display = "block";
      } else {
        card.style.display = "none";
      }
    });
  });

  const addToCartButtons = document.querySelectorAll(".product-card .btn-primary");
  addToCartButtons.forEach((button) => {
    button.addEventListener("click", function () {
      const productCard = this.closest(".product-card");
      const productName = productCard.querySelector("h3").textContent;
      alert(productName + " added to cart!");
    });
  });

  // Cart functionality (mock)
  let cartCount = 0;
  const cartButtons = document.querySelectorAll('[data-action="add-to-cart"]');
  cartButtons.forEach((button) => {
    button.addEventListener("click", function () {
      cartCount++;
      updateCartCount();
    });
  });

  function updateCartCount() {
    const cartElements = document.querySelectorAll(".cart-count");
    cartElements.forEach((el) => {
      el.textContent = cartCount;
    });
  }

  // Newsletter form handling
  const newsletterForm = document.querySelector(".newsletter-form");
  if (newsletterForm) {
    newsletterForm.addEventListener("submit", function (e) {
      e.preventDefault();
      const email = this.querySelector('input[type="email"]').value;
      if (email) {
        alert("Thank you for subscribing!");
        this.reset();
      }
    });
  }

  // Smooth scroll for anchor links
  document.querySelectorAll('a[href^="#"]').forEach((anchor) => {
    anchor.addEventListener("click", function (e) {
      e.preventDefault();
      const target = document.querySelector(this.getAttribute("href"));
      if (target) {
        target.scrollIntoView({
          behavior: "smooth",
        });
      }
    });
  });

  // Lazy load images (mock implementation)
  const lazyImages = document.querySelectorAll("img[data-src]");
  const imageObserver = new IntersectionObserver((entries, observer) => {
    entries.forEach((entry) => {
      if (entry.isIntersecting) {
        const img = entry.target;
        img.src = img.dataset.src;
        img.removeAttribute("data-src");
        observer.unobserve(img);
      }
    });
  });
  lazyImages.forEach((img) => imageObserver.observe(img));

  // Mobile menu toggle
  const mobileMenuButton = document.querySelector(".mobile-menu-btn");
  const mobileMenu = document.querySelector(".mobile-menu");
  if (mobileMenuButton && mobileMenu) {
    mobileMenuButton.addEventListener("click", function () {
      mobileMenu.classList.toggle("hidden");
    });
  }

  // Initialize tooltips
  const tooltipElements = document.querySelectorAll("[data-tooltip]");
  tooltipElements.forEach((el) => {
    el.addEventListener("mouseenter", showTooltip);
    el.addEventListener("mouseleave", hideTooltip);
  });

  function showTooltip(e) {
    const tooltip = document.createElement("div");
    tooltip.className = "tooltip";
    tooltip.textContent = e.target.dataset.tooltip;
    document.body.appendChild(tooltip);
    const rect = e.target.getBoundingClientRect();
    tooltip.style.top = rect.top - 30 + "px";
    tooltip.style.left = rect.left + "px";
    e.target._tooltip = tooltip;
  }

  function hideTooltip(e) {
    if (e.target._tooltip) {
      e.target._tooltip.remove();
    }
  }

  // Analytics tracking (mock)
  function trackEvent(category, action, label) {
    if (typeof gtag !== "undefined") {
      gtag("event", action, {
        event_category: category,
        event_label: label,
      });
    }
    console.log("Track:", category, action, label);
  }

  // Error handling
  window.addEventListener("error", function (e) {
    console.error("Global error:", e.error);
  });

  // Initialize on page load
  window.addEventListener("load", function () {
    console.log("Page loaded successfully");
    trackEvent("Page", "Load", window.location.pathname);
  });
});
