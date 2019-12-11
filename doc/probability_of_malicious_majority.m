lambda = [0.05 0.1 0.2 0.4 0.6];
n = 3:12;
P=zeros(length(lambda), length(n));
for lam = 1:length(lambda)
  for num = 1: length(n)
    % you yourself are honest, so there will always be at least 1 honest actor
    tmp = [ceil(n(num)/2):n(num)-1];
    P(lam,num) = exp(-lambda(lam))*sum(lambda(lam).^tmp./factorial(tmp));
  end
end

semilogy(n, P', 'linewidth', 2)
ylabel("probability to suffer")
xlabel("number of registered meetup participants")

hleg= legend(strvcat(num2str(lambda')));
set(hleg, 'title', 'lambda')
grid on
print -dsvg probability_of_malicious_majority.svg
