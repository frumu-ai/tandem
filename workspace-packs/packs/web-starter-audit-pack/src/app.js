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
});
