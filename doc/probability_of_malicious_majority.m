evilrate = [0.05 0.1 0.2 0.4 0.6];
n = 3:12;
P=zeros(length(evilrate), length(n));
for lam = 1:length(evilrate)
  for num = 1: length(n)
    % you yourself are honest, so there will always be at least 1 honest actor
    tmp = [ceil(n(num)/2):n(num)-1];
    lambda = evilrate(lam)*n(num);
    P(lam,num) = exp(-lambda)*sum(lambda.^tmp./factorial(tmp));
  end
end

semilogy(n, P', 'linewidth', 2)
ylabel("probability to suffer")
xlabel("number of registered meetup participants")

hleg= legend(strvcat(num2str(evilrate')));
set(hleg, 'title', 'malicious')
grid on
print -dsvg probability_of_malicious_majority.svg
